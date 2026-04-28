use dioxus::prelude::*;
use crate::state::{AppState, Section};
use crate::data;
use crate::texts;

#[component]
pub fn ListNotes() -> Element {
    let mut state = use_context::<AppState>();
    let section = state.section.read().clone();

    let folder_id = match &section {
        Section::Folder(id) => Some(*id),
        _ => None,
    };

    let title = match &section {
        Section::Folder(_) => {
            // Look up folder name
            let conn = data::open_db().ok();
            let folders = conn.as_ref().and_then(|c| lore_core::db::list_folders(c).ok()).unwrap_or_default();
            folders.iter().find(|f| Some(f.id) == folder_id).map(|f| f.name.clone()).unwrap_or("Notes".to_string())
        }
        _ => texts::LIST_NOTES.to_string(),
    };

    let mut notes = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::list_notes(&conn, folder_id).unwrap_or_default()
    });

    let tick = state.refresh_tick;
    use_effect(move || {
        let _ = *tick.read();
        let conn = data::open_db().unwrap();
        notes.set(lore_core::db::list_notes(&conn, folder_id).unwrap_or_default());
    });

    let selected = state.selected.read().clone();

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                h2 { class: "list-title", "{title}" }
            }
            div { class: "list-items",
                if notes.read().is_empty() {
                    div { class: "empty-state",
                        if folder_id.is_some() { {texts::EMPTY_FOLDER} } else { {texts::EMPTY_NOTES} }
                    }
                }
                for note in notes.read().iter() {
                    {
                        let is_selected = matches!(&selected, crate::state::Selected::Note(nid) if *nid == note.id);
                        let cls = if is_selected { "list-item selected" } else { "list-item" };
                        let id = note.id;
                        // Title = first line of content (stored in title field),
                        // Preview = beginning of body (second line onwards)
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
                            // Show body preview (already truncated from DB)
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
