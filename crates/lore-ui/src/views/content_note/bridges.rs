//! Hidden `<textarea>` bridges that JS uses to send payloads back to Dioxus:
//!   - markdown content (oninput → save_note + auto-archive URLs)
//!   - attachment download requests
//!   - dropped/picked files
//!   - pasted images
//!
//! Each handler converts the JS payload into Rust data and calls the store.

use dioxus::prelude::*;

use crate::data;
use crate::state::AppState;
use crate::store::DataStore;
use crate::texts;

#[component]
pub fn NoteBridges(id: i64) -> Element {
    rsx! {
        MarkdownBridge { id }
        AttachmentDownloadBridge { id }
        FileDropBridge { id }
        ImagePasteBridge { id }
    }
}

/// Milkdown JS writes the editor's markdown here on every keystroke.
/// We split it into title + body and persist via the store. Must be
/// `<textarea>` (not `<input>`) to preserve newlines.
#[component]
fn MarkdownBridge(id: i64) -> Element {
    let state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();

    rsx! {
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

                store.cleanup_note_attachments(id, &md);

                let urls = lore_core::url_extract::extract_urls(&md);
                if !urls.is_empty() {
                    let space_id = *state.space_id.read();
                    store.auto_archive_urls(&md, space_id);
                    store.set_current_note_urls(urls);
                } else {
                    store.clear_current_note_urls();
                }
            },
        }
    }
}

/// JS sends an attachment id here when the user clicks a file-block link.
/// We open a native save dialog and write the bytes there.
#[component]
fn AttachmentDownloadBridge(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let _ = id; // currently unused — kept in signature to mirror sibling bridges

    rsx! {
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
                    if let Some(h) = handle
                        && h.write(&bytes).await.is_ok()
                    {
                        state.show_toast(texts::TOAST_FILE_SAVED.to_string(), None);
                    }
                });
            },
        }
    }
}

/// JS sends JSON `{name, mime, dataUri}` here when a file is dropped onto the
/// editor. We decode the data URI, upload as attachment, and insert the
/// markdown reference.
#[component]
fn FileDropBridge(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();

    rsx! {
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
                if let Some((name, mime, bytes)) = parsed
                    && let Ok((att_id, outcome)) = store.upload_attachment(id, &name, &mime, &bytes)
                {
                    insert_attachment_ref(&mut store, att_id, &name, &mime);
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
            },
        }
    }
}

/// JS sends a data URI here on image paste. We decode, upload, and insert.
#[component]
fn ImagePasteBridge(id: i64) -> Element {
    let mut store = use_context::<DataStore>();

    rsx! {
        textarea {
            id: "image-bridge",
            style: "position:absolute;left:-9999px;width:1px;height:1px;opacity:0;",
            tabindex: "-1",
            oninput: move |evt| {
                let data_uri = evt.value();
                if !data_uri.starts_with("data:image/") { return; }

                let parts: Vec<&str> = data_uri.splitn(2, ',').collect();
                if parts.len() != 2 { return; }
                let meta = parts[0];
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

                use base64::Engine;
                let bytes = match base64::engine::general_purpose::STANDARD.decode(b64_data) {
                    Ok(b) => b,
                    Err(_) => return,
                };

                let name = format!("paste-{}.{}", chrono::Local::now().format("%H%M%S"), ext);

                if let Ok((att_id, _outcome)) = store.upload_image(id, &name, mime, &bytes) {
                    insert_attachment_ref(&mut store, att_id, &name, mime);
                }
            },
        }
    }
}

/// First line is the title, rest is body. Used by the markdown bridge before
/// persisting via `store.save_note`.
fn split_title_body(text: &str) -> (String, String) {
    match text.split_once('\n') {
        Some((first, rest)) => (first.to_string(), rest.to_string()),
        None => (text.to_string(), String::new()),
    }
}

/// Inject a `[name](https://attachment.lore.invalid/ID)` reference into the
/// editor for `att_id`, then (for images) immediately resolve the URL to a
/// data URI so the image renders without waiting for a re-mount.
///
/// Public-in-module so the actions bar can reuse it when the user picks files
/// via the +Attach button.
pub(super) fn insert_attachment_ref(store: &mut DataStore, att_id: i64, name: &str, mime: &str) {
    let escaped_name = name.replace('\\', "\\\\").replace('\'', "\\'");
    let method = if mime.starts_with("image/") { "insertImage" } else { "insertFile" };
    let js = format!(
        "window.loreEditor && window.loreEditor.{} && window.loreEditor.{}('{}', 'https://attachment.lore.invalid/{}');",
        method, method, escaped_name, att_id
    );
    document::eval(&js);
    if mime.starts_with("image/")
        && let Some(uri) = store.get_attachment_data_uri(att_id)
    {
        let resolve_js = format!(
            "window.loreEditor && window.loreEditor.resolveAttachments({{'{}':'{}'}});",
            att_id,
            uri.replace('\'', "\\'")
        );
        document::eval(&resolve_js);
    }
}

/// Naive JSON string-value extractor: finds `"key":"...."` and returns the
/// (un-escaped) value. Sufficient for the drop-payload schema where the
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
