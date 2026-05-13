//! Action bar at the bottom of a note: Attach file (+hidden file input),
//! Move-to-folder menu, Delete.

use dioxus::prelude::*;

use crate::data;
use crate::state::{AppState, Section, Selected, UndoAction};
use crate::store::DataStore;
use crate::texts;

use super::bridges::insert_attachment_ref;
use super::folder_tree::build_folder_tree;

#[component]
pub fn NoteActions(id: i64, current_folder_id: Option<i64>) -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();
    let mut move_menu_open = use_signal(|| false);

    let folders = use_signal(move || {
        let sid = *state.space_id.read();
        let conn = data::open_db().unwrap();
        lore_core::db::list_folders(&conn, sid).unwrap_or_default()
    });
    let folder_tree: Vec<(i64, String, usize)> = build_folder_tree(&folders.read(), None, 0);

    rsx! {
        // Hidden file input — triggered by the +Attach button below.
        input {
            r#type: "file",
            id: "note-attach-input",
            multiple: true,
            accept: "*/*",
            style: "position:fixed;top:-100px;left:-100px;width:1px;height:1px;opacity:0;pointer-events:none;",
            onchange: move |evt: FormEvent| {
                let files = evt.files();
                if files.is_empty() { return; }
                let note_id = id;
                spawn(async move {
                    let mut store = store;
                    let mut deduped = 0u32;
                    let mut revived = 0u32;
                    for file_data in files {
                        let name = file_data.name();
                        let mime = file_data
                            .content_type()
                            .unwrap_or_else(|| data::mime_from_extension(&name));
                        if let Ok(bytes) = file_data.read_bytes().await
                            && let Ok((att_id, outcome)) = store.upload_attachment(note_id, &name, &mime, &bytes)
                        {
                            match outcome {
                                lore_core::db::InsertAttachmentOutcome::DedupedActive => deduped += 1,
                                lore_core::db::InsertAttachmentOutcome::RevivedFromRemoved => revived += 1,
                                lore_core::db::InsertAttachmentOutcome::Inserted => {}
                            }
                            insert_attachment_ref(&mut store, att_id, &name, &mime);
                        }
                    }
                    store.refresh(&state);
                    if revived > 0 {
                        state.show_toast(texts::TOAST_ATTACHMENT_REVIVED.to_string(), None);
                    } else if deduped > 0 {
                        state.show_toast(texts::TOAST_ATTACHMENT_DEDUPED.to_string(), None);
                    }
                });
            }
        }

        div { class: "note-actions",
            button { class: "btn note-attach-btn",
                r#type: "button",
                onclick: move |_| {
                    document::eval("document.getElementById('note-attach-input').click()");
                },
                "+ Attach file"
            }
            div { class: "move-to-wrapper",
                button { class: "btn",
                    onclick: move |_| move_menu_open.toggle(),
                    {texts::BTN_MOVE_TO}
                }
                if *move_menu_open.read() {
                    div { class: "move-to-menu",
                        if current_folder_id.is_some() {
                            div { class: "move-to-item",
                                onclick: move |_| {
                                    store.move_note(&state, id, None).ok();
                                    move_menu_open.set(false);
                                    state.section.set(Section::AllNotes);
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
                                                store.move_note(&state, id, Some(fid)).ok();
                                                state.section.set(Section::Folder(fid));
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
                        document::eval("if(window.loreEditor) window.loreEditor.destroy();");
                        if store.trash_note(&state, note_id).is_ok() {
                            state.show_toast(
                                texts::TOAST_NOTE_TRASH.to_string(),
                                Some(UndoAction::RestoreNote(note_id)),
                            );
                            state.selected.set(Selected::None);
                        }
                    }
                },
                {texts::BTN_DELETE}
            }
        }
    }
}
