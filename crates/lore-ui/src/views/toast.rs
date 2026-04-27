use dioxus::prelude::*;
use crate::state::{AppState, UndoAction};
use crate::data;
use crate::texts;

#[component]
pub fn Toast() -> Element {
    let mut state = use_context::<AppState>();
    let toast = state.toast.read().clone();

    match toast {
        Some(ref t) => {
            let msg = t.message.clone();
            let has_undo = t.undo.is_some();
            let undo_action = t.undo.clone();

            rsx! {
                div { class: "toast",
                    span { "{msg}" }
                    if has_undo {
                        button { class: "toast-undo",
                            onclick: move |_| {
                                if let Some(ref action) = undo_action {
                                    let conn = data::open_db().unwrap();
                                    match action {
                                        UndoAction::RestorePage(id) => {
                                            lore_core::db::restore_page(&conn, *id).ok();
                                        }
                                        UndoAction::RestoreNote(id) => {
                                            lore_core::db::restore_note(&conn, *id).ok();
                                        }
                                    }
                                    state.bump_refresh();
                                }
                                state.dismiss_toast();
                            },
                            {texts::TOAST_UNDO}
                        }
                    }
                    button { class: "toast-close",
                        onclick: move |_| state.dismiss_toast(),
                        "×"
                    }
                }
            }
        }
        None => rsx! {},
    }
}
