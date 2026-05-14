use crate::backend;
use crate::data;
use crate::state::{AppState, Selected, UndoAction};
use crate::store::DataStore;
use crate::texts;
use dioxus::prelude::*;

#[component]
pub fn ContentFile(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let store = use_context::<DataStore>();

    // `None` = still loading, `Some(None)` = backend confirmed not found,
    // `Some(Some(row))` = loaded.
    let mut file_data = use_signal(|| Option::<Option<lore_core::db::FileRow>>::None);
    let mut data_uri = use_signal(|| Option::<String>::None);

    use_future(move || async move {
        let b = backend::current();
        let row = b.get_file(id).await.ok();
        let uri = match &row {
            Some(f) => {
                let m = f.mime_type.as_deref().unwrap_or("");
                if m.starts_with("image/") || m == "application/pdf" {
                    store.get_file_data_uri(id).await
                } else {
                    None
                }
            }
            None => None,
        };
        file_data.set(Some(row));
        data_uri.set(uri);
    });

    let file_read = file_data.read();
    let Some(loaded) = file_read.as_ref() else {
        return rsx! {
            div { class: "content-panel",
                div { class: "empty-state", "Loading…" }
            }
        };
    };
    let Some(file) = loaded.as_ref() else {
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

            // Action buttons. Save-to-Downloads uses the native file
            // dialog and only renders on desktop; the web build relies
            // on the browser's built-in download UI (TBD via anchor tag).
            div { class: "content-file-actions",
                {
                    #[cfg(feature = "desktop")]
                    {
                        rsx! {
                            button {
                                class: "btn-sm",
                                onclick: move |_| {
                                    spawn(async move {
                                        let b = backend::current();
                                        let Ok(file) = b.get_file(id).await else { return };
                                        let Ok((_, bytes)) = b.get_file_data(id).await else { return };
                                        let default_dir = dirs::download_dir().unwrap_or_default();
                                        let handle = rfd::AsyncFileDialog::new()
                                            .set_file_name(&file.name)
                                            .set_directory(&default_dir)
                                            .save_file()
                                            .await;
                                        if let Some(h) = handle
                                            && h.write(&bytes).await.is_ok() {
                                                state.show_toast(texts::TOAST_FILE_SAVED.to_string(), None);
                                            }
                                    });
                                },
                                {texts::BTN_SAVE_TO_DOWNLOADS}
                            }
                        }
                    }
                    #[cfg(not(feature = "desktop"))]
                    { rsx! {} }
                }
                button {
                    class: "btn-sm btn-danger",
                    onclick: move |_| {
                        let mut store = store;
                        let mut state = state;
                        spawn(async move {
                            if store.trash_file(&state, id).await.is_ok() {
                                state.show_toast(
                                    texts::TOAST_FILE_TRASH.to_string(),
                                    Some(UndoAction::RestoreFile(id)),
                                );
                                state.selected.set(Selected::None);
                            }
                        });
                    },
                    {texts::BTN_DELETE}
                }
            }
        }
    }
}
