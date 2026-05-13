use dioxus::prelude::*;
use crate::state::{AppState, Section};
use crate::store::DataStore;
use crate::texts;

#[component]
pub fn ListNotes() -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();

    let section = state.section.read().clone();
    let folder_id = match &section {
        Section::Folder(id) => Some(*id),
        _ => None,
    };

    let title = match &section {
        Section::Folder(_) => {
            store.folders.read().iter()
                .find(|f| Some(f.id) == folder_id)
                .map(|f| f.name.clone())
                .unwrap_or(texts::LIST_NOTES.to_string())
        }
        _ => texts::LIST_NOTES.to_string(),
    };

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                div { class: "list-header-row",
                    h2 { class: "list-title", "{title}" }
                    button { class: "list-add-btn",
                        onclick: move |_| {
                            if let Ok(note_id) = store.create_note(&state, folder_id) {
                                state.selected.set(crate::state::Selected::Note(note_id));
                            }
                        },
                        "+"
                    }
                }
            }
            div { class: "list-items",
                if store.notes.read().is_empty() {
                    div { class: "empty-state",
                        if folder_id.is_some() { {texts::EMPTY_FOLDER} } else { {texts::EMPTY_NOTES} }
                    }
                }
                for note in store.notes.read().iter() {
                    {
                        let is_selected = matches!(&*state.selected.read(), crate::state::Selected::Note(nid) if *nid == note.id);
                        let cls = if is_selected { "list-item selected" } else { "list-item" };
                        let id = note.id;
                        let display_title = if note.title.is_empty() {
                            if note.body_preview.is_empty() {
                                texts::PLACEHOLDER_NOTE_TITLE.to_string()
                            } else {
                                note.body_preview.lines().next().unwrap_or(texts::PLACEHOLDER_NOTE_TITLE).to_string()
                            }
                        } else {
                            note.title.clone()
                        };
                        let preview = if !note.body_preview.is_empty() {
                            note.body_preview.lines().next().unwrap_or("").to_string()
                        } else {
                            String::new()
                        };
                        rsx! {
                            div { key: "{note.id}", class: "{cls}",
                                onclick: move |_| state.select_note(id),
                                div { class: "list-item-title", "{display_title}" }
                                if !preview.is_empty() {
                                    div { class: "list-item-meta",
                                        span { "{preview}" }
                                    }
                                }
                                div { class: "list-item-date", "{note.updated_at}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
