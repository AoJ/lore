use crate::state::{AppState, UndoAction};
use crate::store::DataStore;
use crate::texts;
use dioxus::prelude::*;
use std::time::Duration;

/// How long a toast stays on screen before auto-dismissing. Undo-bearing
/// toasts get a longer window because the user needs time to read and react.
const AUTO_DISMISS: Duration = Duration::from_secs(4);
const AUTO_DISMISS_WITH_UNDO: Duration = Duration::from_secs(7);

#[component]
pub fn Toast() -> Element {
    let mut state = use_context::<AppState>();
    let store = use_context::<DataStore>();
    let toast = state.toast.read().clone();

    // Per-toast auto-dismiss. `use_effect` tracks the toast id read above
    // (through `toast`), so each new toast restarts the timer instead of
    // sharing a stale one with the previous toast. Manual close (× / Undo)
    // still works through `dismiss_toast()`; the timer just no-ops if the
    // toast got swapped out before its delay elapsed.
    if let Some(ref t) = toast {
        let toast_id = t.id;
        let has_undo = t.undo.is_some();
        use_effect(move || {
            // Read so the effect re-arms whenever id changes.
            let watched_id = toast_id;
            spawn(async move {
                let delay = if has_undo { AUTO_DISMISS_WITH_UNDO } else { AUTO_DISMISS };
                crate::platform::sleep(delay).await;
                // Only dismiss if the toast we armed for is still showing —
                // a newer toast in the meantime owns its own timer.
                let still_owns = state
                    .toast
                    .read()
                    .as_ref()
                    .map(|t| t.id == watched_id)
                    .unwrap_or(false);
                if still_owns {
                    state.dismiss_toast();
                }
            });
        });
    }

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
                                let action = undo_action.clone();
                                spawn(async move {
                                    let mut store = store;
                                    let state = state;
                                    match action {
                                        Some(UndoAction::RestorePage(id)) => { store.restore_page(&state, id).await.ok(); }
                                        Some(UndoAction::RestoreNote(id)) => { store.restore_note(&state, id).await.ok(); }
                                        Some(UndoAction::RestoreFile(id)) => { store.restore_file(&state, id).await.ok(); }
                                        None => {}
                                    }
                                });
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
