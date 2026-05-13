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
    // Bootstrap DB once: opens connection, applies migrations, seeds defaults.
    // If this fails (corrupted DB, schema from newer build, FS permissions, …)
    // we must render an actionable error instead of a blank window.
    let db_path = data::db_path();
    let bootstrap = lore_core::db::open(&db_path);

    rsx! {
        document::Style { {TOKENS_CSS} }
        document::Style { {APP_CSS} }
        document::Style { {EDITOR_CSS} }
        match bootstrap {
            Ok(_conn) => rsx! { BootedApp {} },
            Err(e) => rsx! {
                StartupError {
                    path: db_path.display().to_string(),
                    message: format!("{:#}", e),
                }
            },
        }
    }
}

#[component]
fn BootedApp() -> Element {
    let state = AppState::new();
    let mut store = store::DataStore::new(*state.space_id.read());
    store.refresh(&state);

    use_context_provider(|| state);
    use_context_provider(|| store);

    rsx! {
        script { {MILKDOWN_JS} }
        script { "document.addEventListener('DOMContentLoaded', function() {{ var el = document.querySelector('.app-keyboard-trap'); if (el) el.focus(); }}); setTimeout(function() {{ var el = document.querySelector('.app-keyboard-trap'); if (el) el.focus(); }}, 100);" }
        AppLayout {}
    }
}

#[component]
fn StartupError(path: String, message: String) -> Element {
    rsx! {
        div { class: "startup-error",
            h1 { "lore — startup failed" }
            p { "The database could not be opened." }
            p { class: "startup-error-path", "Path: ", code { "{path}" } }
            pre { class: "startup-error-msg", "{message}" }
            p {
                "If the DB was written by a newer build, install the matching version. "
                "If it's corrupted, restore from a backup or remove it to start fresh "
                "(this will lose data)."
            }
        }
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
    let outdated = *store.schema_outdated.read();
    rsx! {
        div { class: "revision-indicator",
            if outdated {
                span { class: "schema-warning",
                    title: crate::texts::SCHEMA_OUTDATED_TOOLTIP,
                    "{crate::texts::SCHEMA_OUTDATED_LABEL}"
                }
            }
            span { "r{rev}" }
        }
    }
}

fn handle_keyboard(evt: KeyboardEvent, mut state: AppState, mut store: store::DataStore) {
    let key = evt.key();
    let m = evt.modifiers();
    let cmd = m.meta();
    let ctrl = m.ctrl();
    let shift = m.shift();

    let ch = match key {
        Key::Character(ref c) => c.as_str(),
        _ => return,
    };

    match (ch, ctrl, cmd, shift) {
        (c, true, _, _) if c == keys::NAV_DOWN.0 => move_selection(&mut state, 1),
        (c, true, _, _) if c == keys::NAV_UP.0   => move_selection(&mut state, -1),
        ("s", _, true,  _) => save_selected_file(state),
        ("u", _, true,  _) if *state.section.read() == Section::AllFiles => {
            dioxus::document::eval("document.getElementById('file-upload-input').click()");
        }
        ("d", _, true,  _) => trash_selected(&mut state, &mut store),
        ("n", _, true,  false) => create_new_note(&mut state, &mut store),
        ("N", _, true,  true)  => create_new_space(&mut state, &mut store),
        ("F" | "f", _, true, true) => create_new_folder(&mut state, &mut store),
        (c, true, _, _) if is_digit_1_to_9(c) => switch_space_by_index(&mut state, &mut store, c),
        _ => {}
    }
}

fn is_digit_1_to_9(s: &str) -> bool {
    s.len() == 1 && matches!(s.as_bytes()[0], b'1'..=b'9')
}

fn switch_space_by_index(state: &mut AppState, store: &mut store::DataStore, ch: &str) {
    let idx = (ch.as_bytes()[0] - b'1') as usize;
    let Ok(conn) = data::open_db() else { return };
    let spaces = lore_core::db::list_spaces(&conn).unwrap_or_default();
    if let Some(space) = spaces.get(idx) {
        store.switch_space(state, space.id);
    }
}

/// Save the currently-selected file to disk via a native dialog.
/// No-op if selection isn't a file or DB lookup fails.
fn save_selected_file(mut state: AppState) {
    let Selected::File(id) = *state.selected.read() else { return };
    let Ok(conn) = data::open_db() else { return };
    let Ok(file) = lore_core::db::get_file(&conn, id) else { return };
    let Ok((_, bytes)) = lore_core::db::get_file_data(&conn, id) else { return };
    let name = file.name;
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
        if let Some(h) = handle
            && h.write(&bytes).await.is_ok()
        {
            state.show_toast(texts::TOAST_FILE_SAVED.to_string(), None);
        }
    });
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
        Selected::Page(id)
            if store.trash_page(state, id).is_ok() => {
                state.show_toast(texts::TOAST_MOVED_TRASH.to_string(), Some(state::UndoAction::RestorePage(id)));
                state.selected.set(Selected::None);
            }
        Selected::Note(id)
            if store.trash_note(state, id).is_ok() => {
                state.show_toast(texts::TOAST_NOTE_TRASH.to_string(), Some(state::UndoAction::RestoreNote(id)));
                state.selected.set(Selected::None);
            }
        Selected::File(id)
            if store.trash_file(state, id).is_ok() => {
                state.show_toast(texts::TOAST_FILE_TRASH.to_string(), Some(state::UndoAction::RestoreFile(id)));
                state.selected.set(Selected::None);
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
            navigate_ids(&ids, &current, direction, state, Selected::Page);
        }
        Section::AllNotes | Section::Folder(_) => {
            let folder_id = match &section {
                Section::Folder(id) => Some(*id),
                _ => None,
            };
            let ids = note_ids_ordered(folder_id, space_id);
            navigate_ids(&ids, &current, direction, state, Selected::Note);
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
