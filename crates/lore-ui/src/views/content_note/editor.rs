//! Milkdown editor lifecycle: init, drag&drop wiring, attachment URL/metadata
//! pushes, URL-status updates, cleanup-on-unmount. All JS-bridging effects
//! live here so the orchestrator can stay declarative.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::store::DataStore;

/// Mount the editor + attach all lifecycle effects. Renders just the
/// `milkdown-root` div — the bridges (textareas) live in `bridges.rs`.
#[component]
pub fn NoteEditor(id: i64, initial_content: String) -> Element {
    let store = use_context::<DataStore>();

    // Init Milkdown with the note content + push initial URLs to the store
    // so the URL-indicator pipeline is primed before any keystroke.
    {
        let init_content = initial_content.clone();
        let note_id = id;
        let mut store = store;

        let initial_urls = lore_core::url_extract::extract_urls(&init_content);
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

    // Drag&drop wiring on the ProseMirror contenteditable. Polled because
    // Milkdown init timing varies; the element may not exist on first run.
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

    // Resolve initial attachment URLs to data URIs so embedded images/files
    // render correctly on first paint.
    {
        let init_md = initial_content.clone();
        use_effect(move || {
            let mut att_map: HashMap<String, String> = HashMap::new();
            for part in init_md.split("https://attachment.lore.invalid/") {
                if let Some(end_pos) = part.find(')')
                    && let Ok(att_id) = part[..end_pos].parse::<i64>()
                    && let Some(data_uri) = store.get_attachment_data_uri(att_id)
                {
                    att_map.insert(att_id.to_string(), data_uri);
                }
            }
            if !att_map.is_empty() {
                let entries: Vec<String> = att_map
                    .iter()
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

    // Push attachment metadata (size/hash/created_at) so the Milkdown markView
    // renders the rich block (ext badge · name · date · size · hash).
    // Re-runs on revision changes (new uploads, restores, etc.).
    use_effect(move || {
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

    // Cleanup on unmount
    {
        let note_id = id;
        let mut store = store;
        use_drop(move || {
            let js = format!("window.loreEditor && window.loreEditor.cleanup({});", note_id);
            document::eval(&js);
            store.clear_current_note_urls();
        });
    }

    // Push URL statuses to JS editor — re-runs whenever url_statuses changes.
    {
        let statuses_signal = store.url_statuses;
        use_future(move || async move {
            loop {
                let statuses = statuses_signal.read().clone();
                if !statuses.is_empty() {
                    let json_entries: Vec<String> = statuses
                        .iter()
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

    rsx! {
        div { id: "milkdown-root", class: "milkdown-wrapper" }
    }
}
