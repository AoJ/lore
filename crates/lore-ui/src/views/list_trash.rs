use dioxus::prelude::*;
use crate::state::AppState;
use crate::data;
use crate::texts;

#[component]
pub fn ListTrash() -> Element {
    let mut state = use_context::<AppState>();
    let mut items = use_signal(|| data::list_trash().unwrap_or_default());

    let tick = state.refresh_tick;
    use_effect(move || {
        let _ = *tick.read();
        items.set(data::list_trash().unwrap_or_default());
    });

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                h2 { class: "list-title", "{texts::LIST_TRASH} ({items.read().len()})" }
            }
            div { class: "list-items",
                if items.read().is_empty() {
                    div { class: "empty-state", {texts::EMPTY_TRASH} }
                }
                for item in items.read().iter() {
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
                                            let conn = data::open_db().unwrap();
                                            match item_kind {
                                                data::TrashKind::Page => { lore_core::db::restore_page(&conn, item_id).ok(); }
                                                data::TrashKind::Note => { lore_core::db::restore_note(&conn, item_id).ok(); }
                                            }
                                            state.show_toast(texts::TOAST_RESTORED.to_string(), None);
                                            state.bump_refresh();
                                        },
                                        {texts::BTN_RESTORE}
                                    }
                                    button { class: "btn-sm btn-danger",
                                        onclick: move |_| {
                                            let conn = data::open_db().unwrap();
                                            match item_kind2 {
                                                data::TrashKind::Page => { lore_core::db::delete_page(&conn, item_id2).ok(); }
                                                data::TrashKind::Note => { lore_core::db::delete_note_permanent(&conn, item_id2).ok(); }
                                            }
                                            state.bump_refresh();
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
