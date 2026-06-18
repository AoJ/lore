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

use crate::backend;
use crate::state::{AppState, Section};
use crate::store::DataStore;

use actions::NoteActions;
use attachments_panel::RemovedAttachments;
use bridges::NoteBridges;
use editor::NoteEditor;
use folder_tree::folder_path;

#[component]
pub fn ContentNote(id: i64) -> Element {
    let state = use_context::<AppState>();
    let store = use_context::<DataStore>();

    // Outer Option = "loaded yet?", inner Option = "did backend return a note?"
    // — lets us show "Loading…" before the first response and "Note not found"
    // once we know the lookup failed.
    let mut note_data = use_signal(|| Option::<Option<lore_core::db::NoteData>>::None);
    let mut folders = use_signal(Vec::<lore_core::db::FolderRow>::new);

    use_future(move || async move {
        let b = backend::current();
        let n = b.get_note(id).await.ok();
        // Register this note as "open" so the polling loop's
        // external-edit detector knows to push `smartReplace` calls
        // here when the server's `updated_at` advances past what we
        // loaded. Cleared by the `use_drop` below on unmount.
        let mut store = store;
        store.open_note_id.set(Some(id));
        store
            .open_note_updated_at
            .set(n.as_ref().map(|nd| nd.updated_at.clone()));
        note_data.set(Some(n));
        let sid = *state.space_id.read();
        folders.set(b.list_folders(sid).await.unwrap_or_default());
    });

    use_drop(move || {
        let mut store = store;
        store.open_note_id.set(None);
        store.open_note_updated_at.set(None);
    });

    let initial_content = note_data
        .read()
        .as_ref()
        .and_then(|opt| opt.as_ref())
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

    let current_folder_id = note_data
        .read()
        .as_ref()
        .and_then(|opt| opt.as_ref())
        .and_then(|n| n.folder_id);
    let current_folder_name = folder_path(&folders.read(), current_folder_id);

    let note_read = note_data.read();
    let Some(loaded) = note_read.as_ref() else {
        return rsx! {
            div { class: "content-panel",
                div { class: "empty-state", "Loading…" }
            }
        };
    };

    match loaded.as_ref() {
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
                    if let Some(ref import_src) = note.import_source {
                        span { class: "sep", "·" }
                        span {
                            class: "note-import-source",
                            title: "Imported from a markdown file — re-importing the folder keeps it in sync",
                            "↧ {import_src}"
                        }
                    }
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
        None => {
            let offline = !*store.backend_online.read();
            rsx! {
                div { class: "content-panel",
                    p { class: "error",
                        if offline {
                            "No connection to backend — note cannot be loaded."
                        } else {
                            "Note not found."
                        }
                    }
                }
            }
        }
    }
}
