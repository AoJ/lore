use dioxus::prelude::*;

mod state;
mod store;
mod data;
mod texts;
mod keys;
mod views;

use state::{AppState, Section, Selected};
use views::*;

const TOKENS_CSS: &str = include_str!("../assets/tokens.css");
const APP_CSS: &str = include_str!("../assets/app.css");
const MILKDOWN_JS: &str = include_str!("../assets/milkdown.js");
const EDITOR_CSS: &str = include_str!("../assets/editor.css");

fn main() {
    use dioxus::desktop::{Config, WindowBuilder};

    let config = Config::new().with_window(
        WindowBuilder::new()
            .with_title("lore")
            .with_always_on_top(false)
            .with_inner_size(dioxus::desktop::LogicalSize::new(1200.0, 800.0)),
    );

    LaunchBuilder::desktop().with_cfg(config).launch(app);
}

fn app() -> Element {
    let state = AppState::new();
    let mut store = store::DataStore::new(*state.space_id.read());
    store.refresh(&state);

    use_context_provider(|| state);
    use_context_provider(|| store);

    rsx! {
        document::Style { {TOKENS_CSS} }
        document::Style { {APP_CSS} }
        document::Style { {EDITOR_CSS} }
        script { {MILKDOWN_JS} }
        script { "document.addEventListener('DOMContentLoaded', function() {{ var el = document.querySelector('.app-keyboard-trap'); if (el) el.focus(); }}); setTimeout(function() {{ var el = document.querySelector('.app-keyboard-trap'); if (el) el.focus(); }}, 100);" }
        AppLayout {}
    }
}

#[component]
fn AppLayout() -> Element {
    let state = use_context::<AppState>();

    // Read signals INSIDE rsx! so reactivity works — don't clone into locals
    rsx! {
        div { class: "app-layout",
            // Global keyboard handler
            div {
                tabindex: "0",
                autofocus: true,
                class: "app-keyboard-trap",
                onkeydown: move |evt: KeyboardEvent| {
                    let store = use_context::<store::DataStore>();
                    handle_keyboard(evt, state, store);
                },

                // Panel 1: Sidebar
                sidebar::Sidebar {}

                // Panel 2: List
                div { class: "list-panel-container",
                    {match &*state.section.read() {
                        Section::AllPages => rsx! { list_pages::ListPages {} },
                        Section::AllNotes | Section::Folder(_) => rsx! { list_notes::ListNotes {} },
                        Section::AllFiles => rsx! { list_files::ListFiles {} },
                        Section::Search => rsx! { list_search::ListSearch {} },
                        Section::Trash => rsx! { list_trash::ListTrash {} },
                        Section::Timeline => rsx! { list_timeline::ListTimeline {} },
                        Section::Settings => rsx! { list_settings::ListSettings {} },
                    }}
                }

                // Panel 3: Content
                div { class: "content-panel-container",
                    {match &*state.selected.read() {
                        Selected::Page(id) => rsx! { content_page::ContentPage { key: "p-{id}", id: *id } },
                        Selected::Note(id) => rsx! { content_note::ContentNote { key: "n-{id}", id: *id } },
                        Selected::File(id) => rsx! { content_file::ContentFile { key: "f-{id}", id: *id } },
                        Selected::SettingsRules => rsx! { content_rules::ContentRules {} },
                    Selected::SettingsSpaces => rsx! { content_spaces::ContentSpaces {} },
                        Selected::None => rsx! { content_empty::ContentEmpty {} },
                    }}
                }
            }

            // Revision indicator
            RevisionIndicator {}

            // Toast overlay (outside keyboard trap)
            toast::Toast {}
        }
    }
}

/// Central polling loop + revision display. Drives all data refresh.
#[component]
fn RevisionIndicator() -> Element {
    let state = use_context::<AppState>();
    let mut store = use_context::<store::DataStore>();

    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            store.poll(&state);
        }
    });

    let rev = store.revision;
    rsx! {
        div { class: "revision-indicator", "r{rev}" }
    }
}

fn handle_keyboard(evt: KeyboardEvent, mut state: AppState, mut store: store::DataStore) {
    let key = evt.key();
    let modifiers = evt.modifiers();
    let cmd = modifiers.meta();
    let ctrl = modifiers.ctrl();
    let shift = modifiers.shift();

    match key {
        // Ctrl+J / Ctrl+K — navigate list (always works)
        Key::Character(ref ch) if ch == keys::NAV_DOWN.0 && ctrl => {
            move_selection(&mut state, 1);
        }
        Key::Character(ref ch) if ch == keys::NAV_UP.0 && ctrl => {
            move_selection(&mut state, -1);
        }
        // Cmd+S — save selected file to disk via native dialog
        Key::Character(ref ch) if ch == "s" && cmd => {
            if let Selected::File(id) = *state.selected.read() {
                let conn = data::open_db().ok();
                let file_name = conn.as_ref()
                    .and_then(|c| lore_core::db::get_file(c, id).ok())
                    .map(|f| f.name);
                let file_data = conn.as_ref()
                    .and_then(|c| lore_core::db::get_file_data(c, id).ok())
                    .map(|(_, b)| b);
                if let (Some(name), Some(bytes)) = (file_name, file_data) {
                    spawn(async move {
                        // Small delay so WKWebView finishes processing the keydown event
                        // before the native panel takes focus (otherwise dialog flashes).
                        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                        let default_dir = dirs::download_dir().unwrap_or_default();
                        let handle = rfd::AsyncFileDialog::new()
                            .set_file_name(&name)
                            .set_directory(&default_dir)
                            .save_file()
                            .await;
                        if let Some(h) = handle {
                            if h.write(&bytes).await.is_ok() {
                                state.show_toast(texts::TOAST_FILE_SAVED.to_string(), None);
                            }
                        }
                    });
                }
            }
        }
        // Cmd+U — upload file (Files section only)
        Key::Character(ref ch) if ch == "u" && cmd => {
            if *state.section.read() == Section::AllFiles {
                dioxus::document::eval("document.getElementById('file-upload-input').click()");
            }
        }
        // Cmd+D — trash selected
        Key::Character(ref ch) if ch == "d" && cmd => {
            trash_selected(&mut state, &mut store);
        }
        // Cmd+N — new note
        Key::Character(ref ch) if ch == "n" && cmd && !shift => {
            create_new_note(&mut state, &mut store);
        }
        // Cmd+Shift+N — new space
        Key::Character(ref ch) if ch == "N" && cmd && shift => {
            create_new_space(&mut state, &mut store);
        }
        // Cmd+Shift+F — new folder
        Key::Character(ref ch) if (ch == "F" || ch == "f") && cmd && shift => {
            create_new_folder(&mut state, &mut store);
        }
        // Ctrl+1..9 — switch space
        Key::Character(ref ch) if ctrl && ch.len() == 1 && ch.as_bytes()[0] >= b'1' && ch.as_bytes()[0] <= b'9' => {
            let idx = (ch.as_bytes()[0] - b'1') as usize;
            let conn = data::open_db().unwrap();
            let spaces = lore_core::db::list_spaces(&conn).unwrap_or_default();
            if let Some(space) = spaces.get(idx) {
                store.switch_space(&mut state,space.id);
            }
        }
        _ => {}
    }
}

fn create_new_space(state: &mut AppState, store: &mut store::DataStore) {
    if let Ok(new_id) = store.create_space(state, "") {
        store.switch_space(state, new_id);
        state.renaming.set(Some(state::Renaming::Space(new_id, String::new())));
        state.space_dropdown_open.set(true);
    }
}

fn create_new_folder(state: &mut AppState, store: &mut store::DataStore) {
    if let Ok(fid) = store.create_folder(state, "", None) {
        state.renaming.set(Some(state::Renaming::Folder(fid, String::new())));
    }
}

fn create_new_note(state: &mut AppState, store: &mut store::DataStore) {
    let folder_id = match &*state.section.read() {
        Section::Folder(id) => Some(*id),
        _ => None,
    };
    if let Ok(note_id) = store.create_note(state, folder_id) {
        // Switch to Notes section if not already there
        let section = state.section.read().clone();
        if !matches!(section, Section::AllNotes | Section::Folder(_)) {
            state.section.set(Section::AllNotes);
        }
        state.selected.set(Selected::Note(note_id));
        state.bump_refresh();
    }
}

fn trash_selected(state: &mut AppState, store: &mut store::DataStore) {
    let selected = state.selected.read().clone();
    match selected {
        Selected::Page(id) => {
            if store.trash_page(state, id).is_ok() {
                state.show_toast(texts::TOAST_MOVED_TRASH.to_string(), Some(state::UndoAction::RestorePage(id)));
                state.selected.set(Selected::None);
            }
        }
        Selected::Note(id) => {
            if store.trash_note(state, id).is_ok() {
                state.show_toast(texts::TOAST_NOTE_TRASH.to_string(), Some(state::UndoAction::RestoreNote(id)));
                state.selected.set(Selected::None);
            }
        }
        Selected::File(id) => {
            if store.trash_file(state, id).is_ok() {
                state.show_toast(texts::TOAST_FILE_TRASH.to_string(), Some(state::UndoAction::RestoreFile(id)));
                state.selected.set(Selected::None);
            }
        }
        _ => {}
    }
}

/// Move selection up/down in the current list.
/// Uses a single DB query per keypress — gets ordered IDs and finds neighbor.
fn move_selection(state: &mut AppState, direction: i32) {
    let section = state.section.read().clone();
    let current = state.selected.read().clone();

    let space_id = *state.space_id.read();

    match section {
        Section::AllPages => {
            let ids = page_ids_ordered(space_id);
            navigate_ids(&ids, &current, direction, state, |id| Selected::Page(id));
        }
        Section::AllNotes | Section::Folder(_) => {
            let folder_id = match &section {
                Section::Folder(id) => Some(*id),
                _ => None,
            };
            let ids = note_ids_ordered(folder_id, space_id);
            navigate_ids(&ids, &current, direction, state, |id| Selected::Note(id));
        }
        Section::Settings => {
            state.selected.set(Selected::SettingsRules);
        }
        _ => {}
    }
}

fn page_ids_ordered(space_id: i64) -> Vec<i64> {
    let conn = data::open_db().unwrap();
    conn.prepare("SELECT id FROM web_page WHERE trashed_at IS NULL AND space_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 200")
        .and_then(|mut s| {
            let ids: Vec<i64> = s.query_map([space_id], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
            Ok(ids)
        })
        .unwrap_or_default()
}

fn note_ids_ordered(folder_id: Option<i64>, space_id: i64) -> Vec<i64> {
    let conn = data::open_db().unwrap();
    if let Some(fid) = folder_id {
        conn.prepare("SELECT id FROM note WHERE deleted_at IS NULL AND folder_id = ?1 ORDER BY updated_at DESC")
            .and_then(|mut s| {
                let v: Vec<i64> = s.query_map([fid], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
                Ok(v)
            })
            .unwrap_or_default()
    } else {
        conn.prepare("SELECT id FROM note WHERE deleted_at IS NULL AND space_id = ?1 ORDER BY updated_at DESC")
            .and_then(|mut s| {
                let v: Vec<i64> = s.query_map([space_id], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
                Ok(v)
            })
            .unwrap_or_default()
    }
}

fn navigate_ids(
    ids: &[i64],
    current: &Selected,
    direction: i32,
    state: &mut AppState,
    make_selected: impl Fn(i64) -> Selected,
) {
    if ids.is_empty() { return; }
    let current_id = match current {
        Selected::Page(id) | Selected::Note(id) | Selected::File(id) => Some(*id),
        _ => None,
    };
    let current_idx = current_id.and_then(|cid| ids.iter().position(|&id| id == cid));
    let new_idx = match current_idx {
        Some(idx) => (idx as i32 + direction).clamp(0, ids.len() as i32 - 1) as usize,
        None => 0,
    };
    state.selected.set(make_selected(ids[new_idx]));
}
