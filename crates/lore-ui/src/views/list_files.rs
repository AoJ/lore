use dioxus::prelude::*;
use crate::state::{AppState, Selected};
use crate::store::DataStore;
use crate::data;
use crate::texts;

#[component]
pub fn ListFiles() -> Element {
    let mut state = use_context::<AppState>();
    let store = use_context::<DataStore>();

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                div { class: "list-header-row",
                    h2 { class: "list-title", {texts::LIST_FILES} }
                    label { class: "list-add-btn", title: texts::BTN_UPLOAD,
                        r#for: "file-upload-input",
                        "↑"
                    }
                }
            }
            // Hidden file input — triggered by the label above
            input {
                r#type: "file",
                id: "file-upload-input",
                multiple: true,
                accept: "*/*",
                style: "position:fixed;top:-100px;left:-100px;width:1px;height:1px;opacity:0;pointer-events:none;",
                onchange: move |evt: FormEvent| {
                    let files = evt.files();
                    if !files.is_empty() {
                        spawn(async move {
                            let mut store = store;
                            let mut last_id: Option<i64> = None;
                            let mut revived = 0u32;
                            let mut deduped = 0u32;
                            for file_data in files {
                                let name = file_data.name();
                                let mime = file_data.content_type()
                                    .unwrap_or_else(|| data::mime_from_extension(&name));
                                if let Ok(bytes) = file_data.read_bytes().await
                                    && let Ok((id, outcome)) = store.upload_file(&state, &name, Some(&mime), &bytes)
                                {
                                    last_id = Some(id);
                                    match outcome {
                                        lore_core::db::InsertFileOutcome::RevivedFromTrash => revived += 1,
                                        lore_core::db::InsertFileOutcome::DedupedActive => deduped += 1,
                                        lore_core::db::InsertFileOutcome::Inserted => {}
                                    }
                                }
                            }
                            if let Some(id) = last_id {
                                state.selected.set(crate::state::Selected::File(id));
                            }
                            if revived > 0 {
                                state.show_toast(texts::TOAST_FILE_RESTORED_FROM_TRASH.to_string(), None);
                            } else if deduped > 0 {
                                state.show_toast(texts::TOAST_FILE_DEDUPED.to_string(), None);
                            }
                        });
                    }
                }
            }
            div { class: "list-items",
                if store.files.read().is_empty() {
                    div { class: "empty-state", {texts::EMPTY_FILES} }
                }
                for file in store.files.read().iter() {
                    {
                        let file_id = file.id;
                        let ext = data::file_extension(&file.name);
                        let size = data::format_file_size(file.size);
                        let short_hash = file.hash.chars().take(8).collect::<String>();
                        let date = file.created_at.clone();
                        let name = file.name.clone();
                        let is_selected = *state.selected.read() == Selected::File(file_id);

                        rsx! {
                            div {
                                key: "{file_id}",
                                class: if is_selected { "list-item selected" } else { "list-item" },
                                onclick: move |_| {
                                    state.selected.set(Selected::File(file_id));
                                },
                                div { class: "file-item-row",
                                    span { class: "file-ext-badge", "{ext}" }
                                    span { class: "list-item-title file-name", "{name}" }
                                }
                                div { class: "list-item-meta",
                                    span { "{date}" }
                                    span { class: "sep", "·" }
                                    span { "{size}" }
                                    span { class: "sep", "·" }
                                    span { class: "file-hash", "{short_hash}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
