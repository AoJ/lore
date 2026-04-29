use dioxus::prelude::*;
use crate::state::{AppState, Selected, UndoAction};
use crate::data;
use crate::texts;

fn format_size(bytes: i64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

fn ext_from_name(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "FILE".into())
}

/// Naive JSON string-value extractor: finds `"key":"...."` and returns the
/// (un-escaped) value. Sufficient for our drop-payload schema where the
/// producer is our own JS using `JSON.stringify`.
fn json_extract(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = json.find(&needle)? + needle.len();
    let rest = &json[start..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                other => out.push(other),
            }
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

#[component]
pub fn ContentNote(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<crate::store::DataStore>();
    let mut move_menu_open = use_signal(|| false);

    let note_data = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::get_note(&conn, id).ok()
    });

    let initial_content = note_data.read().as_ref().map(|n| {
        if n.title.is_empty() && n.body.is_empty() {
            String::new()
        } else if n.body.is_empty() {
            n.title.clone()
        } else {
            format!("{}\n{}", n.title, n.body)
        }
    }).unwrap_or_default();

    let mut content = use_signal(move || initial_content);

    // Initialize Milkdown with note ID + set initial URLs
    {
        let init_content = content.read().clone();
        let note_id = id;

        // Set URLs for initial content
        let initial_urls = data::extract_urls(&init_content);
        if !initial_urls.is_empty() {
            store.set_current_note_urls(initial_urls);
        }

        use_effect(move || {
            let escaped = init_content
                .replace('\\', "\\\\")
                .replace('`', "\\`")
                .replace("</", "<\\/");
            let js = format!(
                "window.loreEditor && window.loreEditor.init('milkdown-root', `{}`, 'milkdown-bridge', {});",
                escaped, note_id
            );
            document::eval(&js);
        });
    }

    // Wire up drag&drop and attachment-link click interceptor on the editor.
    // Uses a polling pattern instead of fixed setTimeout because Milkdown init
    // timing varies between versions and the editor's contenteditable element
    // may not exist when this effect first runs.
    {
        use_effect(move || {
            let js = r#"
                (function bind(tries) {
                    var root = document.getElementById('milkdown-root');
                    var pm = root && (root.querySelector('.ProseMirror') || root.querySelector('[contenteditable]'));
                    if (!pm) {
                        if (tries > 0) setTimeout(function(){bind(tries-1);}, 150);
                        return;
                    }
                    if (pm._loreFileDropBound) return;
                    pm._loreFileDropBound = true;

                    // Drag&drop: file into editor → upload as attachment
                    pm.addEventListener('dragover', function(e) {
                        if (e.dataTransfer && e.dataTransfer.types && e.dataTransfer.types.indexOf('Files') !== -1) {
                            e.preventDefault();
                        }
                    });
                    pm.addEventListener('drop', function(e) {
                        if (!e.dataTransfer || !e.dataTransfer.files || e.dataTransfer.files.length === 0) return;
                        e.preventDefault();
                        var bridge = document.getElementById('file-bridge');
                        if (!bridge) return;
                        var setter = Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value').set;
                        Array.from(e.dataTransfer.files).forEach(function(file) {
                            var reader = new FileReader();
                            reader.onload = function(ev) {
                                var payload = JSON.stringify({
                                    name: file.name,
                                    mime: file.type || 'application/octet-stream',
                                    dataUri: ev.target.result
                                });
                                setter.call(bridge, payload);
                                bridge.dispatchEvent(new Event('input', {bubbles: true}));
                            };
                            reader.readAsDataURL(file);
                        });
                    });
                    // Click handling for attachment blocks is provided by the
                    // Milkdown markView (js/index.js → buildLinkMarkView).
                })(40);
            "#;
            document::eval(js);
        });
    }

    // Resolve attachment URLs after editor init
    {
        let init_md = content.read().clone();
        use_effect(move || {
            // Find all https://attachment.lore.invalid/ID references and resolve to data URIs
            let mut att_map = std::collections::HashMap::new();
            for part in init_md.split("https://attachment.lore.invalid/") {
                if let Some(end_pos) = part.find(')') {
                    if let Ok(att_id) = part[..end_pos].parse::<i64>() {
                        if let Some(data_uri) = store.get_attachment_data_uri(att_id) {
                            att_map.insert(att_id.to_string(), data_uri);
                        }
                    }
                }
            }
            if !att_map.is_empty() {
                let entries: Vec<String> = att_map.iter()
                    .map(|(k, v)| format!("'{}':'{}'", k, v.replace('\'', "\\'")))
                    .collect();
                let js = format!(
                    "setTimeout(function(){{ window.loreEditor && window.loreEditor.resolveAttachments({{{}}}); }}, 500);",
                    entries.join(",")
                );
                document::eval(&js);
            }
        });
    }

    // Push attachment metadata (size/hash/created_at) to the editor so the
    // markView can render the rich block (ext badge · name · date · size · hash).
    // Re-runs on revision changes (new uploads, restores, etc.).
    {
        use_effect(move || {
            // Subscribe to revision changes
            let _rev = *store.revision.read();

            let active = store.list_active_attachments(id);
            let removed = store.list_removed_attachments(id);
            let mut entries: Vec<String> = Vec::new();
            for att in active.iter().chain(removed.iter()) {
                let esc = |s: &str| s.replace('\\', "\\\\").replace('\'', "\\'");
                entries.push(format!(
                    "'{}':{{name:'{}',size:{},hash:'{}',created_at:'{}',mime_type:'{}'}}",
                    att.id,
                    esc(&att.name),
                    att.size,
                    esc(&att.hash),
                    esc(&att.created_at),
                    esc(att.mime_type.as_deref().unwrap_or("")),
                ));
            }
            if !entries.is_empty() {
                let js = format!(
                    "(function poll(tries){{ \
                        if (window.loreEditor && window.loreEditor.setAttachmentMeta) {{ \
                            window.loreEditor.setAttachmentMeta({{{}}}); \
                        }} else if (tries > 0) {{ \
                            setTimeout(function(){{poll(tries-1);}}, 150); \
                        }} \
                    }})(20);",
                    entries.join(",")
                );
                document::eval(&js);
            }
        });
    }

    // Cleanup on unmount
    {
        let note_id = id;
        use_drop(move || {
            let js = format!("window.loreEditor && window.loreEditor.cleanup({});", note_id);
            document::eval(&js);
            store.clear_current_note_urls();
        });
    }

    // Push URL statuses to JS editor — re-runs whenever url_statuses signal changes
    {
        let statuses_signal = store.url_statuses;
        use_future(move || async move {
            loop {
                // Read signal (subscribes to changes)
                let statuses = statuses_signal.read().clone();
                if !statuses.is_empty() {
                    let json_entries: Vec<String> = statuses.iter()
                        .map(|(url, status)| format!("\"{}\":\"{}\"", url.replace('"', "\\\""), status))
                        .collect();
                    let json = format!("{{{}}}", json_entries.join(","));
                    // Delay to let Milkdown render links first
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let js = format!("window.loreEditor && window.loreEditor.updateUrlStatuses({});", json);
                    document::eval(&js);
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
    }

    let folders = use_signal(move || {
        let sid = *state.space_id.read();
        let conn = data::open_db().unwrap();
        lore_core::db::list_folders(&conn, sid).unwrap_or_default()
    });

    let folder_tree: Vec<(i64, String, usize)> = build_folder_tree(&folders.read(), None, 0);
    let current_folder_id = note_data.read().as_ref().and_then(|n| n.folder_id);
    let current_folder_name = folder_path(&folders.read(), current_folder_id);

    match note_data.read().as_ref() {
        Some(note) => rsx! {
            section { class: "content-panel content-note",
                // Dirty indicator (unsaved changes)
                div { id: "dirty-indicator", class: "dirty-indicator",
                    style: "opacity: 0;",
                    "●"
                }
                // Milkdown editor
                div { id: "milkdown-root", class: "milkdown-wrapper" }

                // Bridge textarea: Milkdown JS writes markdown here → Dioxus reads it
                // Must be textarea (not input) to preserve newlines in markdown
                textarea {
                    id: "milkdown-bridge",
                    "data-note-id": "{id}",
                    style: "position:absolute;left:-9999px;width:1px;height:1px;opacity:0;",
                    tabindex: "-1",
                    oninput: move |evt| {
                        let md = evt.value();
                        if md.is_empty() { return; }

                        let (title, body) = split_title_body(&md);
                        store.save_note(id, &title, &body).ok();

                        // Cleanup orphaned attachments
                        store.cleanup_note_attachments(id, &md);

                        // Extract URLs, auto-archive, update indicators
                        let urls = data::extract_urls(&md);
                        if !urls.is_empty() {
                            let space_id = *state.space_id.read();
                            store.auto_archive_urls(&md, space_id);
                            store.set_current_note_urls(urls);
                        } else {
                            store.clear_current_note_urls();
                        }
                    },
                }

                // Attachment download bridge — JS sends attachment id when user
                // clicks a https://attachment.lore.invalid/ link in the editor body.
                textarea {
                    id: "att-download-bridge",
                    style: "position:absolute;left:-9999px;width:1px;height:1px;opacity:0;",
                    tabindex: "-1",
                    oninput: move |evt| {
                        let payload = evt.value();
                        let att_id: i64 = match payload.parse() {
                            Ok(n) => n,
                            Err(_) => return,
                        };
                        let conn = match data::open_db() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        let row = match lore_core::db::get_attachment(&conn, att_id) {
                            Ok(r) => r,
                            Err(_) => return,
                        };
                        let bytes = match lore_core::db::get_attachment_data(&conn, att_id) {
                            Ok((_, b)) => b,
                            Err(_) => return,
                        };
                        let fname = row.name;
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                            let default_dir = dirs::download_dir().unwrap_or_default();
                            let handle = rfd::AsyncFileDialog::new()
                                .set_file_name(&fname)
                                .set_directory(&default_dir)
                                .save_file()
                                .await;
                            if let Some(h) = handle {
                                if h.write(&bytes).await.is_ok() {
                                    state.show_toast(texts::TOAST_FILE_SAVED.to_string(), None);
                                }
                            }
                        });
                    },
                }

                // File drop bridge — JS sends JSON {name, mime, dataUri} here
                textarea {
                    id: "file-bridge",
                    style: "position:absolute;left:-9999px;width:1px;height:1px;opacity:0;",
                    tabindex: "-1",
                    oninput: move |evt| {
                        let payload = evt.value();
                        if payload.is_empty() { return; }
                        let parsed: Option<(String, String, Vec<u8>)> = (|| {
                            let s = payload.as_str();
                            let name = json_extract(s, "name")?;
                            let mime = json_extract(s, "mime")?;
                            let data_uri = json_extract(s, "dataUri")?;
                            let comma = data_uri.find(',')?;
                            let b64 = &data_uri[comma + 1..];
                            use base64::Engine;
                            let bytes = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
                            Some((name, mime, bytes))
                        })();
                        if let Some((name, mime, bytes)) = parsed {
                            if let Ok((att_id, outcome)) = store.upload_attachment(id, &name, &mime, &bytes) {
                                let escaped_name = name.replace('\\', "\\\\").replace('\'', "\\'");
                                let method = if mime.starts_with("image/") { "insertImage" } else { "insertFile" };
                                let js = format!(
                                    "window.loreEditor && window.loreEditor.{} && window.loreEditor.{}('{}', 'https://attachment.lore.invalid/{}');",
                                    method, method, escaped_name, att_id
                                );
                                document::eval(&js);
                                if mime.starts_with("image/") {
                                    if let Some(uri) = store.get_attachment_data_uri(att_id) {
                                        let resolve_js = format!(
                                            "window.loreEditor && window.loreEditor.resolveAttachments({{'{}':'{}'}});",
                                            att_id,
                                            uri.replace('\'', "\\'")
                                        );
                                        document::eval(&resolve_js);
                                    }
                                }
                                match outcome {
                                    lore_core::db::InsertAttachmentOutcome::DedupedActive => {
                                        state.show_toast(texts::TOAST_ATTACHMENT_DEDUPED.to_string(), None);
                                    }
                                    lore_core::db::InsertAttachmentOutcome::RevivedFromRemoved => {
                                        state.show_toast(texts::TOAST_ATTACHMENT_REVIVED.to_string(), None);
                                    }
                                    lore_core::db::InsertAttachmentOutcome::Inserted => {}
                                }
                                store.refresh(&state);
                            }
                        }
                    },
                }

                // Image paste bridge — JS sends data URI here
                textarea {
                    id: "image-bridge",
                    style: "position:absolute;left:-9999px;width:1px;height:1px;opacity:0;",
                    tabindex: "-1",
                    oninput: move |evt| {
                        let data_uri = evt.value();
                        if !data_uri.starts_with("data:image/") { return; }

                        // Parse data URI: data:image/png;base64,AAAA...
                        let parts: Vec<&str> = data_uri.splitn(2, ',').collect();
                        if parts.len() != 2 { return; }
                        let meta = parts[0]; // data:image/png;base64
                        let b64_data = parts[1];

                        let mime = meta
                            .strip_prefix("data:")
                            .and_then(|s| s.split(';').next())
                            .unwrap_or("image/png");

                        let ext = match mime {
                            "image/png" => "png",
                            "image/jpeg" | "image/jpg" => "jpg",
                            "image/gif" => "gif",
                            "image/webp" => "webp",
                            _ => "png",
                        };

                        // Decode base64
                        use base64::Engine;
                        let bytes = match base64::engine::general_purpose::STANDARD.decode(b64_data) {
                            Ok(b) => b,
                            Err(_) => return,
                        };

                        let name = format!("paste-{}.{}", chrono::Local::now().format("%H%M%S"), ext);

                        // Upload to DB
                        if let Ok((att_id, _outcome)) = store.upload_image(id, &name, mime, &bytes) {
                            // Insert markdown into editor
                            let js = format!(
                                "window.loreEditor && window.loreEditor.insertImage('{}', 'https://attachment.lore.invalid/{}');",
                                name, att_id
                            );
                            document::eval(&js);

                            // Resolve immediately so image displays
                            if let Some(data_uri) = store.get_attachment_data_uri(att_id) {
                                let resolve_js = format!(
                                    "window.loreEditor && window.loreEditor.resolveAttachments({{'{}':'{}'}});",
                                    att_id,
                                    data_uri.replace('\'', "\\'")
                                );
                                document::eval(&resolve_js);
                            }
                        }
                    },
                }

                div { class: "note-footer",
                    span { "Created: {note.created_at}" }
                    span { class: "sep", "·" }
                    span { "Modified: {note.updated_at}" }
                    if let Some(ref folder_path_str) = current_folder_name {
                        span { class: "sep", "·" }
                        span { class: "note-folder-link",
                            onclick: {
                                let fid = current_folder_id.unwrap();
                                move |_| store.navigate(&mut state,crate::state::Section::Folder(fid))
                            },
                            "📁 {folder_path_str}"
                        }
                    }
                }

                // Hidden file input for attachments
                input {
                    r#type: "file",
                    id: "note-attach-input",
                    multiple: true,
                    accept: "*/*",
                    style: "position:fixed;top:-100px;left:-100px;width:1px;height:1px;opacity:0;pointer-events:none;",
                    onchange: move |evt: FormEvent| {
                        let files = evt.files();
                        if files.is_empty() { return; }
                        let note_id = id;
                        spawn(async move {
                            let mut store = store;
                            let mut deduped = 0u32;
                            let mut revived = 0u32;
                            for file_data in files {
                                let name = file_data.name();
                                let mime = file_data.content_type()
                                    .unwrap_or_else(|| data::mime_from_extension(&name));
                                if let Ok(bytes) = file_data.read_bytes().await {
                                    if let Ok((att_id, outcome)) = store.upload_attachment(note_id, &name, &mime, &bytes) {
                                        match outcome {
                                            lore_core::db::InsertAttachmentOutcome::DedupedActive => deduped += 1,
                                            lore_core::db::InsertAttachmentOutcome::RevivedFromRemoved => revived += 1,
                                            lore_core::db::InsertAttachmentOutcome::Inserted => {}
                                        }
                                        let escaped_name = name.replace('\'', "\\'").replace('\\', "\\\\");
                                        let method = if mime.starts_with("image/") { "insertImage" } else { "insertFile" };
                                        let js = format!(
                                            "window.loreEditor && window.loreEditor.{} && window.loreEditor.{}('{}', 'https://attachment.lore.invalid/{}');",
                                            method, method, escaped_name, att_id
                                        );
                                        document::eval(&js);
                                        if mime.starts_with("image/") {
                                            if let Some(uri) = store.get_attachment_data_uri(att_id) {
                                                let resolve_js = format!(
                                                    "window.loreEditor && window.loreEditor.resolveAttachments({{'{}':'{}'}});",
                                                    att_id,
                                                    uri.replace('\'', "\\'")
                                                );
                                                document::eval(&resolve_js);
                                            }
                                        }
                                    }
                                }
                            }
                            store.refresh(&state);
                            if revived > 0 {
                                state.show_toast(texts::TOAST_ATTACHMENT_REVIVED.to_string(), None);
                            } else if deduped > 0 {
                                state.show_toast(texts::TOAST_ATTACHMENT_DEDUPED.to_string(), None);
                            }
                        });
                    }
                }

                // Removed attachments section (active ones render inline as blocks
                // via the Milkdown markView, no need to repeat them at the bottom).
                {
                    let _rev = *store.revision.read();
                    let removed = store.list_removed_attachments(id);
                    rsx! {
                        if !removed.is_empty() {
                            div { class: "note-attachments",
                                div { class: "note-attachments-header", "Removed (auto-delete after 30 days)" }
                                div { class: "note-attachments-list",
                                    for att in removed.iter() {
                                        {
                                            let aid = att.id;
                                            let aname = att.name.clone();
                                            let ext = ext_from_name(&att.name);
                                            let size = format_size(att.size);
                                            let date = att.deleted_at.clone().unwrap_or_default()
                                                .chars().take(10).collect::<String>();
                                            let short_hash = att.hash.chars().take(8).collect::<String>();
                                            rsx! {
                                                div { key: "r-{aid}", class: "attachment-row removed",
                                                    span { class: "file-ext-badge", "{ext}" }
                                                    span { class: "attachment-name", title: "{aname}", "{att.name}" }
                                                    span { class: "attachment-meta",
                                                        "{date}"
                                                        span { class: "sep", "·" }
                                                        "{size}"
                                                        span { class: "sep", "·" }
                                                        span { class: "file-hash", "{short_hash}" }
                                                    }
                                                    div { class: "attachment-actions",
                                                        button { class: "btn-sm",
                                                            onclick: move |_| {
                                                                if let Ok(row) = store.restore_attachment(&state, aid) {
                                                                    let mime = row.mime_type.unwrap_or_default();
                                                                    let prefix = if mime.starts_with("image/") { "!" } else { "" };
                                                                    let escaped = row.name.replace('\'', "\\'").replace('\\', "\\\\");
                                                                    let method = if mime.starts_with("image/") { "insertImage" } else { "insertFile" };
                                                                    let _ = prefix; // syntax built by method choice
                                                                    let js = format!(
                                                                        "window.loreEditor && window.loreEditor.{} && window.loreEditor.{}('{}', 'https://attachment.lore.invalid/{}');",
                                                                        method, method, escaped, aid
                                                                    );
                                                                    document::eval(&js);
                                                                    if mime.starts_with("image/") {
                                                                        if let Some(uri) = store.get_attachment_data_uri(aid) {
                                                                            let resolve_js = format!(
                                                                                "window.loreEditor && window.loreEditor.resolveAttachments({{'{}':'{}'}});",
                                                                                aid,
                                                                                uri.replace('\'', "\\'")
                                                                            );
                                                                            document::eval(&resolve_js);
                                                                        }
                                                                    }
                                                                    state.show_toast(texts::TOAST_ATTACHMENT_RESTORED.to_string(), None);
                                                                }
                                                            },
                                                            {texts::BTN_RESTORE}
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                div { class: "note-actions",
                    button { class: "btn note-attach-btn",
                        r#type: "button",
                        onclick: move |_| {
                            document::eval("document.getElementById('note-attach-input').click()");
                        },
                        "+ Attach file"
                    }
                    div { class: "move-to-wrapper",
                        button { class: "btn",
                            onclick: move |_| move_menu_open.toggle(),
                            {texts::BTN_MOVE_TO}
                        }
                        if *move_menu_open.read() {
                            div { class: "move-to-menu",
                                if current_folder_id.is_some() {
                                    div { class: "move-to-item",
                                        onclick: move |_| {
                                            store.move_note(&state, id, None).ok();
                                            move_menu_open.set(false);
                                            state.section.set(crate::state::Section::AllNotes);
                                            state.bump_refresh();
                                        },
                                        {texts::MOVE_TO_ROOT}
                                    }
                                }
                                for (fid, fname, depth) in folder_tree.iter() {
                                    {
                                        let fid = *fid;
                                        let depth = *depth;
                                        let is_current = Some(fid) == current_folder_id;
                                        let indent = format!("{}rem", depth as f32 * 0.75);
                                        let label = fname.clone();
                                        let cls = if is_current { "move-to-item current" } else { "move-to-item" };
                                        rsx! {
                                            div { key: "{fid}", class: "{cls}",
                                                style: "padding-left: calc(var(--spacing-sm) + {indent})",
                                                onclick: move |_| {
                                                    if !is_current {
                                                        store.move_note(&state, id, Some(fid)).ok();
                                                        state.section.set(crate::state::Section::Folder(fid));
                                                        state.bump_refresh();
                                                    }
                                                    move_menu_open.set(false);
                                                },
                                                "📁 {label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    button { class: "btn btn-danger",
                        onclick: {
                            let note_id = id;
                            move |_| {
                                document::eval("if(window.loreEditor) window.loreEditor.destroy();");
                                if store.trash_note(&state, note_id).is_ok() {
                                    state.show_toast(
                                        texts::TOAST_NOTE_TRASH.to_string(),
                                        Some(UndoAction::RestoreNote(note_id)),
                                    );
                                    state.selected.set(Selected::None);
                                }
                            }
                        },
                        {texts::BTN_DELETE}
                    }
                }
            }
        },
        None => rsx! {
            div { class: "content-panel",
                p { class: "error", "Note not found" }
            }
        },
    }
}

fn split_title_body(text: &str) -> (String, String) {
    match text.split_once('\n') {
        Some((first, rest)) => (first.to_string(), rest.to_string()),
        None => (text.to_string(), String::new()),
    }
}

fn build_folder_tree(
    folders: &[lore_core::db::FolderRow],
    parent_id: Option<i64>,
    depth: usize,
) -> Vec<(i64, String, usize)> {
    let mut result = Vec::new();
    for f in folders.iter().filter(|f| f.parent_id == parent_id) {
        result.push((f.id, f.name.clone(), depth));
        result.extend(build_folder_tree(folders, Some(f.id), depth + 1));
    }
    result
}

fn folder_path(folders: &[lore_core::db::FolderRow], folder_id: Option<i64>) -> Option<String> {
    let fid = folder_id?;
    let mut parts = Vec::new();
    let mut current = fid;
    loop {
        let folder = folders.iter().find(|f| f.id == current)?;
        parts.push(folder.name.clone());
        match folder.parent_id {
            Some(pid) => current = pid,
            None => break,
        }
    }
    parts.reverse();
    Some(parts.join(" / "))
}

fn auto_archive_urls(text: &str, space_id: i64) {
    let conn = match data::open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    let rules = lore_core::db::load_rules(&conn).unwrap_or_default();

    // Extract URLs from markdown links [text](url) and bare URLs
    let mut urls = Vec::new();

    // Pattern 1: [text](url)
    let mut rest = text;
    while let Some(pos) = rest.find("](") {
        let start = pos + 2;
        if let Some(end) = rest[start..].find(')') {
            let url = rest[start..start + end].trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                urls.push(url.to_string());
            }
            rest = &rest[start + end..];
        } else {
            break;
        }
    }

    // Pattern 2: bare URLs (https://... not inside markdown link)
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| c == '(' || c == ')' || c == '<' || c == '>');
        if (word.starts_with("http://") || word.starts_with("https://")) && !urls.contains(&word.to_string()) {
            urls.push(word.to_string());
        }
    }

    for url in &urls {
        if lore_core::db::find_page_by_url(&conn, url).ok().flatten().is_none() {
            if let Ok(parsed) = url::Url::parse(url) {
                let normalized = lore_core::rules::normalize_url(&parsed);
                let domain = parsed.host_str().unwrap_or("unknown").to_string();
                let category = lore_core::rules::classify(&parsed, &rules);
                let status = if category == "archive" { "queued" } else { "skipped" };
                lore_core::db::insert_web_page(&conn, &lore_core::db::NewWebPage {
                    url,
                    url_normalized: &normalized,
                    title: None,
                    domain: &domain,
                    category: &category,
                    status,
                    source: Some("note"),
                    space_id: Some(space_id),
                }).ok();
            }
        }
    }
}
