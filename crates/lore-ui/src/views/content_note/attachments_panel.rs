//! Removed-attachments panel: soft-deleted files for this note, with a
//! Restore button. Auto-deleted permanently after 30 days via the trash
//! cleanup task.

use dioxus::prelude::*;

use crate::data;
use crate::state::AppState;
use crate::store::DataStore;
use crate::texts;

use super::bridges::insert_attachment_ref;

#[component]
pub fn RemovedAttachments(id: i64) -> Element {
    let state = use_context::<AppState>();
    let store = use_context::<DataStore>();

    // Subscribe to revision changes so restores from elsewhere refresh us too;
    // `use_future` fetches the removed-list each time the revision bumps.
    let revision = store.revision;
    let mut removed = use_signal(Vec::<lore_core::db::AttachmentRow>::new);
    use_future(move || async move {
        let _rev = *revision.read();
        removed.set(store.list_removed_attachments(id).await);
    });

    if removed.read().is_empty() {
        return rsx! {};
    }

    rsx! {
        div { class: "note-attachments",
            div { class: "note-attachments-header", "Removed (auto-delete after 30 days)" }
            div { class: "note-attachments-list",
                for att in removed.read().iter() {
                    {
                        let aid = att.id;
                        let aname = att.name.clone();
                        let ext = data::file_extension(&att.name);
                        let size = data::format_file_size(att.size);
                        let date = att
                            .deleted_at
                            .clone()
                            .unwrap_or_default()
                            .chars()
                            .take(10)
                            .collect::<String>();
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
                                            let mut store = store;
                                            let mut state = state;
                                            spawn(async move {
                                                if let Ok(row) = store.restore_attachment(&state, aid).await {
                                                    let mime = row.mime_type.unwrap_or_default();
                                                    insert_attachment_ref(&mut store, aid, &row.name, &mime).await;
                                                    state.show_toast(
                                                        texts::TOAST_ATTACHMENT_RESTORED.to_string(),
                                                        None,
                                                    );
                                                }
                                            });
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
