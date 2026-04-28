use dioxus::prelude::*;
use crate::state::{AppState, Selected, UndoAction};
use crate::data;
use crate::texts;

#[component]
pub fn ContentNote(id: i64) -> Element {
    let mut state = use_context::<AppState>();

    let note_data = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::get_note(&conn, id).ok()
    });

    // Single text buffer: first line = title, rest = body
    let mut content = use_signal(|| {
        note_data.read().as_ref().map(|n| {
            if n.title.is_empty() && n.body.is_empty() {
                String::new()
            } else if n.body.is_empty() {
                n.title.clone()
            } else {
                format!("{}\n{}", n.title, n.body)
            }
        }).unwrap_or_default()
    });

    let mut save = move || {
        let text = content.read().clone();
        let (title, body) = split_title_body(&text);
        let conn = data::open_db().unwrap();
        lore_core::db::update_note(&conn, id, &title, &body).ok();
        state.bump_refresh();
    };

    match note_data.read().as_ref() {
        Some(note) => rsx! {
            section { class: "content-panel content-note",
                textarea {
                    class: "note-editor",
                    placeholder: texts::PLACEHOLDER_NOTE_BODY,
                    value: "{content}",
                    oninput: move |evt| {
                        content.set(evt.value());
                        save();
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
                                state.selected.set(Selected::None);
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

/// Split content into title (first line) and body (remaining lines).
fn split_title_body(text: &str) -> (String, String) {
    match text.split_once('\n') {
        Some((first, rest)) => (first.to_string(), rest.to_string()),
        None => (text.to_string(), String::new()),
    }
}
