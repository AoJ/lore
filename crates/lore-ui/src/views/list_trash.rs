use dioxus::prelude::*;
use crate::state::AppState;
use crate::store::DataStore;
use crate::data;
use crate::texts;

#[component]
pub fn ListTrash() -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                h2 { class: "list-title", "{texts::LIST_TRASH} ({store.trash_items.read().len()})" }
            }
            div { class: "list-items",
                if store.trash_items.read().is_empty() {
                    div { class: "empty-state", {texts::EMPTY_TRASH} }
                }
                for item in store.trash_items.read().iter() {
                    {
                        let kind_label = match item.kind {
                            data::TrashKind::Page => texts::KIND_PAGE,
                            data::TrashKind::Note => texts::KIND_NOTE,
                        };
                        let item_id = item.id;
                        let item_kind = item.kind.clone();
                        let item_id2 = item.id;
                        let item_kind2 = item.kind.clone();
                        rsx! {
                            div { key: "{kind_label}-{item.id}", class: "list-item trash-item",
                                div { class: "list-item-title", "{item.title}" }
                                div { class: "list-item-meta",
                                    span { "{kind_label}" }
                                    span { class: "sep", "·" }
                                    span { "{item.trashed_at}" }
                                }
                                div { class: "trash-actions",
                                    button { class: "btn-sm",
                                        onclick: move |_| {
                                            let result = match item_kind {
                                                data::TrashKind::Page => store.restore_page(&state, item_id),
                                                data::TrashKind::Note => store.restore_note(&state, item_id),
                                            };
                                            if result.is_ok() {
                                                state.show_toast(texts::TOAST_RESTORED.to_string(), None);
                                            }
                                        },
                                        {texts::BTN_RESTORE}
                                    }
                                    button { class: "btn-sm btn-danger",
                                        onclick: move |_| {
                                            match item_kind2 {
                                                data::TrashKind::Page => { store.delete_page_permanent(&state, item_id2).ok(); }
                                                data::TrashKind::Note => { store.delete_note_permanent(&state, item_id2).ok(); }
                                            }
                                        },
                                        {texts::BTN_DELETE_FOREVER}
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
