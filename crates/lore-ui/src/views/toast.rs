use crate::state::{AppState, UndoAction};
use crate::store::DataStore;
use crate::texts;
use dioxus::prelude::*;

#[component]
pub fn Toast() -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();
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
                                    match action {
                                        UndoAction::RestorePage(id) => {
                                            store.restore_page(&state, *id).ok();
                                        }
                                        UndoAction::RestoreNote(id) => {
                                            store.restore_note(&state, *id).ok();
                                        }
                                        UndoAction::RestoreFile(id) => {
                                            store.restore_file(&state, *id).ok();
                                        }
                                    }
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
