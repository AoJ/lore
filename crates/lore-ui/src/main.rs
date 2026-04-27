use dioxus::prelude::*;

mod state;
mod data;
mod texts;
mod views;

use state::{AppState, Section, Selected};
use views::*;

const TOKENS_CSS: &str = include_str!("../assets/tokens.css");
const APP_CSS: &str = include_str!("../assets/app.css");

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
    use_context_provider(|| state);

    rsx! {
        document::Style { {TOKENS_CSS} }
        document::Style { {APP_CSS} }
        // Auto-focus the keyboard trap on load
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
                    handle_keyboard(evt, state);
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
                        Selected::None => rsx! { content_empty::ContentEmpty {} },
                    }}
                }
            }

            // Toast overlay (outside keyboard trap)
            toast::Toast {}
        }
    }
}

fn handle_keyboard(evt: KeyboardEvent, mut state: AppState) {
    let key = evt.key();
    let modifiers = evt.modifiers();
    let cmd = modifiers.meta();

    let ctrl = modifiers.ctrl();

    match key {
        Key::Character(ref ch) if ch == "j" && ctrl => {
            move_selection(&mut state, 1);
        }
        Key::Character(ref ch) if ch == "k" && ctrl => {
            move_selection(&mut state, -1);
        }
        Key::Backspace if !cmd => {
            if *state.selected.read() != Selected::None {
                state.selected.set(Selected::None);
            }
        }
        Key::Character(ref ch) if ch == "d" && cmd => {
            trash_selected(&mut state);
        }
        _ => {}
    }
}

fn trash_selected(state: &mut AppState) {
    let selected = state.selected.read().clone();
    match selected {
        Selected::Page(id) => {
            let conn = data::open_db().unwrap();
            lore_core::db::trash_page(&conn, id).ok();
            state.show_toast(
                texts::TOAST_MOVED_TRASH.to_string(),
                Some(state::UndoAction::RestorePage(id)),
            );
            state.selected.set(Selected::None);
            state.bump_refresh();
        }
        Selected::Note(id) => {
            let conn = data::open_db().unwrap();
            lore_core::db::trash_note(&conn, id).ok();
            state.show_toast(
                texts::TOAST_NOTE_TRASH.to_string(),
                Some(state::UndoAction::RestoreNote(id)),
            );
            state.selected.set(Selected::None);
            state.bump_refresh();
        }
        _ => {}
    }
}

/// Move selection up/down in the current list.
/// Uses a single DB query per keypress — gets ordered IDs and finds neighbor.
fn move_selection(state: &mut AppState, direction: i32) {
    let section = state.section.read().clone();
    let current = state.selected.read().clone();

    match section {
        Section::AllPages => {
            let ids = page_ids_ordered();
            navigate_ids(&ids, &current, direction, state, |id| Selected::Page(id));
        }
        Section::AllNotes | Section::Folder(_) => {
            let folder_id = match &section {
                Section::Folder(id) => Some(*id),
                _ => None,
            };
            let ids = note_ids_ordered(folder_id);
            navigate_ids(&ids, &current, direction, state, |id| Selected::Note(id));
        }
        Section::Settings => {
            state.selected.set(Selected::SettingsRules);
        }
        _ => {}
    }
}

fn page_ids_ordered() -> Vec<i64> {
    let conn = data::open_db().unwrap();
    conn.prepare("SELECT id FROM web_page WHERE trashed_at IS NULL ORDER BY created_at DESC, id DESC LIMIT 200")
        .and_then(|mut s| {
            let ids: Vec<i64> = s.query_map([], |r| r.get(0)).ok()
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default();
            Ok(ids)
        })
        .unwrap_or_default()
}

fn note_ids_ordered(folder_id: Option<i64>) -> Vec<i64> {
    let conn = data::open_db().unwrap();
    if let Some(fid) = folder_id {
        conn.prepare("SELECT id FROM note WHERE deleted_at IS NULL AND folder_id = ?1 ORDER BY updated_at DESC")
            .and_then(|mut s| {
                let v: Vec<i64> = s.query_map([fid], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
                Ok(v)
            })
            .unwrap_or_default()
    } else {
        conn.prepare("SELECT id FROM note WHERE deleted_at IS NULL ORDER BY updated_at DESC")
            .and_then(|mut s| {
                let v: Vec<i64> = s.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
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
