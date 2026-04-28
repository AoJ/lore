use dioxus::prelude::*;
use crate::state::{AppState, Selected, UndoAction};
use crate::data;
use crate::texts;

#[component]
pub fn ContentNote(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let mut move_menu_open = use_signal(|| false);

    let note_data = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::get_note(&conn, id).ok()
    });

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

    // Load folders for move menu — build flat list with depth for tree display
    let folders = use_signal(move || {
        let sid = *state.space_id.read();
        let conn = data::open_db().unwrap();
        lore_core::db::list_folders(&conn, sid).unwrap_or_default()
    });

    let folder_tree: Vec<(i64, String, usize)> = build_folder_tree(&folders.read(), None, 0);

    let current_folder_id = note_data.read().as_ref().and_then(|n| n.folder_id);
    let current_folder_name = folder_path(&folders.read(), current_folder_id);

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
                    if let Some(ref folder_path_str) = current_folder_name {
                        span { class: "sep", "·" }
                        span { class: "note-folder-link",
                            onclick: {
                                let fid = current_folder_id.unwrap();
                                move |_| state.navigate(crate::state::Section::Folder(fid))
                            },
                            "📁 {folder_path_str}"
                        }
                    }
                }
                div { class: "note-actions",
                    // Move to folder
                    div { class: "move-to-wrapper",
                        button { class: "btn",
                            onclick: move |_| move_menu_open.toggle(),
                            "Move to..."
                        }
                        if *move_menu_open.read() {
                            div { class: "move-to-menu",
                                if current_folder_id.is_some() {
                                    div { class: "move-to-item",
                                        onclick: move |_| {
                                            let conn = data::open_db().unwrap();
                                            lore_core::db::move_note_to_folder(&conn, id, None).ok();
                                            move_menu_open.set(false);
                                            state.section.set(crate::state::Section::AllNotes);
                                            state.bump_refresh();
                                        },
                                        {texts::MOVE_TO_ROOT}
                                    }
                                }
                                for (fid, fname, depth) in folder_tree.iter() {
                                    {
                                        let fid = *fid;
                                        let depth = *depth;
                                        let is_current = Some(fid) == current_folder_id;
                                        let indent = format!("{}rem", depth as f32 * 0.75);
                                        let label = fname.clone();
                                        let cls = if is_current { "move-to-item current" } else { "move-to-item" };
                                        rsx! {
                                            div { key: "{fid}", class: "{cls}",
                                                style: "padding-left: calc(var(--spacing-sm) + {indent})",
                                                onclick: move |_| {
                                                    if !is_current {
                                                        let conn = data::open_db().unwrap();
                                                        lore_core::db::move_note_to_folder(&conn, id, Some(fid)).ok();
                                                        state.section.set(crate::state::Section::Folder(fid));
                                                        state.bump_refresh();
                                                    }
                                                    move_menu_open.set(false);
                                                },
                                                "📁 {label}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
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

fn split_title_body(text: &str) -> (String, String) {
    match text.split_once('\n') {
        Some((first, rest)) => (first.to_string(), rest.to_string()),
        None => (text.to_string(), String::new()),
    }
}

/// Build a flat list of (id, name, depth) from hierarchical folders, sorted as a tree.
fn build_folder_tree(
    folders: &[lore_core::db::FolderRow],
    parent_id: Option<i64>,
    depth: usize,
) -> Vec<(i64, String, usize)> {
    let mut result = Vec::new();
    for f in folders.iter().filter(|f| f.parent_id == parent_id) {
        result.push((f.id, f.name.clone(), depth));
        result.extend(build_folder_tree(folders, Some(f.id), depth + 1));
    }
    result
}

/// Build full path for a folder: "Parent / Child / Grandchild"
fn folder_path(folders: &[lore_core::db::FolderRow], folder_id: Option<i64>) -> Option<String> {
    let fid = folder_id?;
    let mut parts = Vec::new();
    let mut current = fid;
    loop {
        let folder = folders.iter().find(|f| f.id == current)?;
        parts.push(folder.name.clone());
        match folder.parent_id {
            Some(pid) => current = pid,
            None => break,
        }
    }
    parts.reverse();
    Some(parts.join(" / "))
}
