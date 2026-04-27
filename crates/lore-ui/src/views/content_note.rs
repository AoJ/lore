use dioxus::prelude::*;
use crate::state::{AppState, UndoAction};
use crate::data;
use crate::texts;

#[component]
pub fn ContentNote(id: i64) -> Element {
    let mut state = use_context::<AppState>();

    let note_data = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::get_note(&conn, id).ok()
    });

    let mut title = use_signal(|| {
        note_data.read().as_ref().map(|n| n.title.clone()).unwrap_or_default()
    });
    let mut body = use_signal(|| {
        note_data.read().as_ref().map(|n| n.body.clone()).unwrap_or_default()
    });

    // Auto-save on changes
    let mut save_note = move || {
        let t = title.read().clone();
        let b = body.read().clone();
        let conn = data::open_db().unwrap();
        lore_core::db::update_note(&conn, id, &t, &b).ok();
        state.bump_refresh();
    };

    match note_data.read().as_ref() {
        Some(note) => rsx! {
            section { class: "content-panel content-note",
                input {
                    class: "note-title",
                    r#type: "text",
                    placeholder: texts::PLACEHOLDER_NOTE_TITLE,
                    value: "{title}",
                    oninput: move |evt| {
                        title.set(evt.value());
                        save_note();
                    },
                }
                hr { class: "note-divider" }
                textarea {
                    class: "note-body",
                    placeholder: texts::PLACEHOLDER_NOTE_BODY,
                    value: "{body}",
                    oninput: move |evt| {
                        body.set(evt.value());
                        save_note();
                    },
                }
                div { class: "note-footer",
                    span { "Created: {note.created_at}" }
                    span { class: "sep", "·" }
                    span { "Modified: {note.updated_at}" }
                }
                div { class: "note-actions",
                    button { class: "btn btn-danger",
                        onclick: {
                            let note_id = id;
                            move |_| {
                                let conn = data::open_db().unwrap();
                                lore_core::db::trash_note(&conn, note_id).ok();
                                state.show_toast(
                                    texts::TOAST_NOTE_TRASH.to_string(),
                                    Some(UndoAction::RestoreNote(note_id)),
                                );
                                state.selected.set(crate::state::Selected::None);
                                state.bump_refresh();
                            }
                        },
                        {texts::BTN_DELETE}
                    }
                }
            }
        },
        None => rsx! {
            div { class: "content-panel",
                p { class: "error", "Note not found" }
            }
        },
    }
}
