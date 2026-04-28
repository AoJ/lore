use dioxus::prelude::*;
use crate::state::{AppState, Section};
use crate::data;
use crate::texts;

#[component]
pub fn ListNotes() -> Element {
    let mut state = use_context::<AppState>();
    let mut notes = use_signal(Vec::<lore_core::db::NoteRow>::new);
    let mut panel_title = use_signal(|| texts::LIST_NOTES.to_string());

    // Read section signal inside effect so it re-runs on every section change
    let section_signal = state.section;
    let space_signal = state.space_id;
    let tick = state.refresh_tick;

    use_effect(move || {
        let _ = *tick.read();
        let section = section_signal.read().clone();
        let sid = *space_signal.read();

        let folder_id = match &section {
            Section::Folder(id) => Some(*id),
            _ => None,
        };

        // Update title
        let title = match &section {
            Section::Folder(_) => {
                let conn = data::open_db().ok();
                let folders = conn.as_ref().and_then(|c| lore_core::db::list_folders(c, sid).ok()).unwrap_or_default();
                folders.iter().find(|f| Some(f.id) == folder_id).map(|f| f.name.clone()).unwrap_or(texts::LIST_NOTES.to_string())
            }
            _ => texts::LIST_NOTES.to_string(),
        };
        panel_title.set(title);

        // Fetch notes
        let conn = data::open_db().unwrap();
        notes.set(lore_core::db::list_notes(&conn, folder_id, sid).unwrap_or_default());
    });

    // Revision-based polling — update list when DB changes (e.g. note edited in content panel)
    use_future(move || async move {
        let mut last_rev = data::get_revision();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let current_rev = data::get_revision();
            if current_rev != last_rev {
                last_rev = current_rev;
                let section = section_signal.read().clone();
                let sid = *space_signal.read();
                let folder_id = match &section {
                    Section::Folder(id) => Some(*id),
                    _ => None,
                };
                let conn = data::open_db().unwrap();
                notes.set(lore_core::db::list_notes(&conn, folder_id, sid).unwrap_or_default());
            }
        }
    });

    // Current folder_id for the "+" button
    let current_folder_id = match &*state.section.read() {
        Section::Folder(id) => Some(*id),
        _ => None,
    };

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                div { class: "list-header-row",
                    h2 { class: "list-title", "{panel_title}" }
                    button { class: "list-add-btn",
                        onclick: move |_| {
                            let sid = *state.space_id.read();
                            let fid = match &*state.section.read() {
                                Section::Folder(id) => Some(*id),
                                _ => None,
                            };
                            let conn = data::open_db().unwrap();
                            if let Ok(note_id) = lore_core::db::insert_note(&conn, "", "", fid, sid) {
                                state.selected.set(crate::state::Selected::Note(note_id));
                                state.bump_refresh();
                            }
                        },
                        "+"
                    }
                }
            }
            div { class: "list-items",
                if notes.read().is_empty() {
                    div { class: "empty-state",
                        if current_folder_id.is_some() { {texts::EMPTY_FOLDER} } else { {texts::EMPTY_NOTES} }
                    }
                }
                for note in notes.read().iter() {
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
