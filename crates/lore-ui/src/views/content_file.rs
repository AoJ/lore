use crate::data;
use crate::state::{AppState, Selected, UndoAction};
use crate::store::DataStore;
use crate::texts;
use dioxus::prelude::*;

#[component]
pub fn ContentFile(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();

    let conn = data::open_db().ok();
    let file = conn
        .as_ref()
        .and_then(|c| lore_core::db::get_file(c, id).ok());

    let Some(file) = file else {
        return rsx! {
            div { class: "content-panel",
                div { class: "empty-state", "File not found." }
            }
        };
    };

    let mime = file
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");
    let is_image = mime.starts_with("image/");
    let is_pdf = mime == "application/pdf";

    let ext = data::file_extension(&file.name);
    let size = data::format_file_size(file.size);
    let short_hash = file.hash.chars().take(8).collect::<String>();

    // Compute data URI for preview (images and PDFs only)
    let data_uri = use_memo(move || {
        let conn = data::open_db().ok()?;
        let f = lore_core::db::get_file(&conn, id).ok()?;
        let m = f.mime_type.as_deref().unwrap_or("");
        if m.starts_with("image/") || m == "application/pdf" {
            store.get_file_data_uri(id)
        } else {
            None
        }
    });

    rsx! {
        div { class: "content-panel content-file",
            // Header: extension badge + filename
            div { class: "content-file-header",
                span { class: "file-ext-badge file-ext-large", "{ext}" }
                div { class: "content-file-title", "{file.name}" }
            }

            // Metadata row
            div { class: "content-file-meta",
                span { "{mime}" }
                span { class: "sep", "·" }
                span { "{size}" }
                span { class: "sep", "·" }
                span { "{file.created_at}" }
                span { class: "sep", "·" }
                span { class: "file-hash", "SHA-256: {short_hash}…" }
            }

            hr { class: "content-divider" }

            // Preview
            div { class: "content-file-preview",
                if is_image {
                    if let Some(uri) = data_uri.read().as_ref() {
                        img {
                            class: "file-preview-image",
                            src: "{uri}",
                            alt: "{file.name}"
                        }
                    } else {
                        div { class: "file-no-preview", "Loading preview…" }
                    }
                } else if is_pdf {
                    if let Some(uri) = data_uri.read().as_ref() {
                        object {
                            class: "file-preview-pdf",
                            data: "{uri}",
                            r#type: "application/pdf",
                            p { "PDF preview not available in this view." }
                        }
                    } else {
                        div { class: "file-no-preview", "Loading PDF…" }
                    }
                } else {
                    div { class: "file-no-preview",
                        div { class: "file-icon-large", "{ext}" }
                        p { "No preview available for this file type." }
                    }
                }
            }

            // Action buttons
            div { class: "content-file-actions",
                button {
                    class: "btn-sm",
                    onclick: move |_| {
                        let conn = data::open_db().ok();
                        let file_name = conn.as_ref()
                            .and_then(|c| lore_core::db::get_file(c, id).ok())
                            .map(|f| f.name);
                        let file_data = conn.as_ref()
                            .and_then(|c| lore_core::db::get_file_data(c, id).ok())
                            .map(|(_, b)| b);
                        if let (Some(name), Some(bytes)) = (file_name, file_data) {
                            spawn(async move {
                                let default_dir = dirs::download_dir().unwrap_or_default();
                                let handle = rfd::AsyncFileDialog::new()
                                    .set_file_name(&name)
                                    .set_directory(&default_dir)
                                    .save_file()
                                    .await;
                                if let Some(h) = handle
                                    && h.write(&bytes).await.is_ok() {
                                        state.show_toast(texts::TOAST_FILE_SAVED.to_string(), None);
                                    }
                            });
                        }
                    },
                    {texts::BTN_SAVE_TO_DOWNLOADS}
                }
                button {
                    class: "btn-sm btn-danger",
                    onclick: move |_| {
                        if store.trash_file(&state, id).is_ok() {
                            state.show_toast(
                                texts::TOAST_FILE_TRASH.to_string(),
                                Some(UndoAction::RestoreFile(id)),
                            );
                            state.selected.set(Selected::None);
                        }
                    },
                    {texts::BTN_DELETE}
                }
            }
        }
    }
}
