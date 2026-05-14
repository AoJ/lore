//! Note detail view. The `ContentNote` component composes:
//!   - `NoteEditor`        — Milkdown JS bridge + lifecycle effects
//!   - `NoteBridges`       — hidden textareas JS writes to (markdown, files, …)
//!   - `RemovedAttachments`— soft-deleted attachments with Restore
//!   - `NoteActions`       — +Attach, Move-to-folder, Delete
//!
//! Helpers (folder tree, etc.) live in `folder_tree.rs`.

mod actions;
mod attachments_panel;
mod bridges;
mod editor;
mod folder_tree;

use dioxus::prelude::*;

use crate::data;
use crate::state::{AppState, Section};
use crate::store::DataStore;

use actions::NoteActions;
use attachments_panel::RemovedAttachments;
use bridges::NoteBridges;
use editor::NoteEditor;
use folder_tree::folder_path;

#[component]
pub fn ContentNote(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();

    let note_data = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::get_note(&conn, id).ok()
    });

    let initial_content = note_data
        .read()
        .as_ref()
        .map(|n| {
            if n.title.is_empty() && n.body.is_empty() {
                String::new()
            } else if n.body.is_empty() {
                n.title.clone()
            } else {
                format!("{}\n{}", n.title, n.body)
            }
        })
        .unwrap_or_default();

    let folders = use_signal(move || {
        let sid = *state.space_id.read();
        let conn = data::open_db().unwrap();
        lore_core::db::list_folders(&conn, sid).unwrap_or_default()
    });
    let current_folder_id = note_data.read().as_ref().and_then(|n| n.folder_id);
    let current_folder_name = folder_path(&folders.read(), current_folder_id);

    match note_data.read().as_ref() {
        Some(note) => rsx! {
            section { class: "content-panel content-note",
                // Dirty indicator (unsaved changes — updated by JS)
                div { id: "dirty-indicator", class: "dirty-indicator",
                    style: "opacity: 0;",
                    "●"
                }

                NoteEditor { id, initial_content: initial_content.clone() }
                NoteBridges { id }

                div { class: "note-footer",
                    span { "Created: {note.created_at}" }
                    span { class: "sep", "·" }
                    span { "Modified: {note.updated_at}" }
                    if let Some(ref folder_path_str) = current_folder_name {
                        span { class: "sep", "·" }
                        span { class: "note-folder-link",
                            onclick: {
                                let fid = current_folder_id.unwrap();
                                move |_| {
                                    let mut store = store;
                                    let mut state = state;
                                    spawn(async move { store.navigate(&mut state, Section::Folder(fid)).await; });
                                }
                            },
                            "📁 {folder_path_str}"
                        }
                    }
                }

                RemovedAttachments { id }
                NoteActions { id, current_folder_id }
            }
        },
        None => rsx! {
            div { class: "content-panel",
                p { class: "error", "Note not found" }
            }
        },
    }
}
