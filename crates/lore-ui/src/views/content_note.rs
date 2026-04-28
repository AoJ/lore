use dioxus::prelude::*;
use crate::state::{AppState, Selected, UndoAction};
use crate::data;
use crate::texts;

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

    // Resolve attachment URLs after editor init
    {
        let init_md = content.read().clone();
        use_effect(move || {
            // Find all lore://attachment/ID references and resolve to data URIs
            let mut att_map = std::collections::HashMap::new();
            for part in init_md.split("lore://attachment/") {
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

    // Cleanup on unmount
    {
        let note_id = id;
        use_drop(move || {
            let js = format!("window.loreEditor && window.loreEditor.cleanup({});", note_id);
            document::eval(&js);
            store.clear_current_note_urls();
        });
    }

    // Push URL statuses to JS editor when they change
    {
        let url_statuses_signal = store.url_statuses;
        use_effect(move || {
            let statuses = url_statuses_signal.read();
            if statuses.is_empty() { return; }
            // Serialize to JSON and call JS
            let json_entries: Vec<String> = statuses.iter()
                .map(|(url, status)| format!("\"{}\":\"{}\"", url.replace('"', "\\\""), status))
                .collect();
            let json = format!("{{{}}}", json_entries.join(","));
            let js = format!("window.loreEditor && window.loreEditor.updateUrlStatuses({});", json);
            document::eval(&js);
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
                        if let Ok(att_id) = store.upload_image(id, &name, mime, &bytes) {
                            // Insert markdown into editor
                            let js = format!(
                                "window.loreEditor && window.loreEditor.insertImage('{}', 'lore://attachment/{}');",
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

                div { class: "note-actions",
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
