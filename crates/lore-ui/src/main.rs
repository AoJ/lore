use dioxus::prelude::*;

mod backend;
mod data;
mod keys;
mod platform;
mod state;
mod store;
mod texts;
mod views;

use state::{AppState, Section, Selected};
use std::sync::Arc;
use views::*;

const TOKENS_CSS: &str = include_str!("../assets/tokens.css");
const APP_CSS: &str = include_str!("../assets/app.css");
const MILKDOWN_JS: &str = include_str!("../assets/milkdown.js");
const EDITOR_CSS: &str = include_str!("../assets/editor.css");

#[cfg(feature = "desktop")]
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

#[cfg(feature = "web")]
fn main() {
    // The web build relies on the server (`lore-server`) for the data
    // layer. The browser fetches the WASM bundle, mounts the same `app`
    // root, and the `HttpBackend` initialized in `app()` calls back to
    // `/api/*` on the same origin.
    dioxus::launch(app);
}

#[cfg(feature = "desktop")]
fn app() -> Element {
    // Bootstrap DB once: opens connection, applies migrations, seeds defaults.
    // If this fails (corrupted DB, schema from newer build, FS permissions, …)
    // we must render an actionable error instead of a blank window.
    let db_path = data::db_path();
    let bootstrap = lore_core::db::open(&db_path);

    // On successful bootstrap, install the process-wide backend so every
    // mutation/refresh has somewhere to send DB calls. Done before the first
    // component renders.
    if bootstrap.is_ok() {
        backend::init(Arc::new(backend::LocalBackend::new(db_path.clone())));
    }

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

#[cfg(feature = "web")]
fn app() -> Element {
    // No bootstrap step on web — the server already ran `lore_core::db::open`.
    // We install the `HttpBackend` once and hand off to the same component
    // tree the desktop uses. Relative `/api` base means the WASM bundle and
    // the API live behind the same origin (typical reverse-proxy setup).
    backend::init(Arc::new(backend::HttpBackend::new("/api".to_string())));

    rsx! {
        document::Style { {TOKENS_CSS} }
        document::Style { {APP_CSS} }
        document::Style { {EDITOR_CSS} }
        BootedApp {}
    }
}

#[component]
fn BootedApp() -> Element {
    let state = AppState::new();
    let store = store::DataStore::new();

    use_context_provider(|| state);
    use_context_provider(|| store);

    // Initial load: pick the most recently used non-deleted space and run
    // the first refresh. AppState started with `space_id = 1`, so we'd
    // display the seeded default until this future resolves.
    use_future(move || {
        let mut state = state;
        let mut store = store;
        async move {
            if let Ok(active) = backend::current().get_active_space().await {
                state.space_id.set(active.id);
            }
            store.refresh(&state).await;
        }
    });

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
    let store = use_context::<store::DataStore>();

    let offline = !*store.backend_online.read();
    let layout_class = if offline {
        "app-layout offline"
    } else {
        "app-layout"
    };

    rsx! {
        // Offline banner sits above the app in document flow — pushes content
        // down rather than overlapping it.
        OfflineBanner {}
        div { class: "{layout_class}",
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
            crate::platform::sleep(std::time::Duration::from_secs(2)).await;
            store.poll(&state).await;
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

/// Shown when the backend is unreachable. Keeps data visible but warns that
/// writes are not persisted. Disappears automatically on reconnect.
#[component]
fn OfflineBanner() -> Element {
    let store = use_context::<store::DataStore>();
    if *store.backend_online.read() {
        return rsx! {};
    }
    rsx! {
        div { class: "offline-banner",
            span { class: "offline-banner-icon", "⚠" }
            span { "Backend unavailable — data is read-only. Changes will not be saved until the connection is restored." }
        }
    }
}

/// Sync keyboard dispatcher. Async work is fired into a background task via
/// `spawn`; the handler itself returns immediately so the keypress isn't
/// blocked.
fn handle_keyboard(evt: KeyboardEvent, state: AppState, store: store::DataStore) {
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
        (c, true, _, _) if c == keys::NAV_DOWN.0 => move_selection(state, 1),
        (c, true, _, _) if c == keys::NAV_UP.0 => move_selection(state, -1),
        #[cfg(feature = "desktop")]
        ("s", _, true, _) => save_selected_file(state),
        ("u", _, true, _) if *state.section.read() == Section::AllFiles => {
            dioxus::document::eval("document.getElementById('file-upload-input').click()");
        }
        ("d", _, true, _) => trash_selected(state, store),
        ("n", _, true, false) => create_new_note(state, store),
        ("N", _, true, true) => create_new_space(state, store),
        ("F" | "f", _, true, true) => create_new_folder(state, store),
        (c, true, _, _) if is_digit_1_to_9(c) => switch_space_by_index(state, store, c),
        _ => {}
    }
}

fn is_digit_1_to_9(s: &str) -> bool {
    s.len() == 1 && matches!(s.as_bytes()[0], b'1'..=b'9')
}

fn switch_space_by_index(mut state: AppState, mut store: store::DataStore, ch: &str) {
    let idx = (ch.as_bytes()[0] - b'1') as usize;
    spawn(async move {
        let spaces = backend::current().list_spaces().await.unwrap_or_default();
        if let Some(space) = spaces.get(idx) {
            store.switch_space(&mut state, space.id).await;
        }
    });
}

/// Save the currently-selected file to disk via a native dialog.
/// No-op if selection isn't a file or DB lookup fails. Desktop-only —
/// the web variant relies on the browser's built-in download UI (anchor
/// link click), to be wired up alongside the W3 web port.
#[cfg(feature = "desktop")]
fn save_selected_file(mut state: AppState) {
    let Selected::File(id) = *state.selected.read() else {
        return;
    };
    spawn(async move {
        let b = backend::current();
        let Ok(file) = b.get_file(id).await else {
            return;
        };
        let Ok((_, bytes)) = b.get_file_data(id).await else {
            return;
        };
        let name = file.name;
        // Small delay so WKWebView finishes processing the keydown event
        // before the native panel takes focus (otherwise dialog flashes).
        crate::platform::sleep(std::time::Duration::from_millis(80)).await;
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

fn create_new_space(mut state: AppState, mut store: store::DataStore) {
    spawn(async move {
        if let Ok(new_id) = store.create_space(&state, "").await {
            store.switch_space(&mut state, new_id).await;
            state
                .renaming
                .set(Some(state::Renaming::Space(new_id, String::new())));
            state.space_dropdown_open.set(true);
        }
    });
}

fn create_new_folder(state: AppState, mut store: store::DataStore) {
    let mut state = state;
    spawn(async move {
        if let Ok(fid) = store.create_folder(&state, "", None).await {
            state
                .renaming
                .set(Some(state::Renaming::Folder(fid, String::new())));
        }
    });
}

fn create_new_note(mut state: AppState, mut store: store::DataStore) {
    spawn(async move {
        let folder_id = match &*state.section.read() {
            Section::Folder(id) => Some(*id),
            _ => None,
        };
        if let Ok(note_id) = store.create_note(&state, folder_id).await {
            // Switch to Notes section if not already there
            let section = state.section.read().clone();
            if !matches!(section, Section::AllNotes | Section::Folder(_)) {
                state.section.set(Section::AllNotes);
            }
            state.selected.set(Selected::Note(note_id));
            state.bump_refresh();
        }
    });
}

fn trash_selected(mut state: AppState, mut store: store::DataStore) {
    let selected = state.selected.read().clone();
    spawn(async move {
        match selected {
            Selected::Page(id) if store.trash_page(&state, id).await.is_ok() => {
                state.show_toast(
                    texts::TOAST_MOVED_TRASH.to_string(),
                    Some(state::UndoAction::RestorePage(id)),
                );
                state.selected.set(Selected::None);
            }
            Selected::Note(id) if store.trash_note(&state, id).await.is_ok() => {
                state.show_toast(
                    texts::TOAST_NOTE_TRASH.to_string(),
                    Some(state::UndoAction::RestoreNote(id)),
                );
                state.selected.set(Selected::None);
            }
            Selected::File(id) if store.trash_file(&state, id).await.is_ok() => {
                state.show_toast(
                    texts::TOAST_FILE_TRASH.to_string(),
                    Some(state::UndoAction::RestoreFile(id)),
                );
                state.selected.set(Selected::None);
            }
            _ => {}
        }
    });
}

/// Move selection up/down in the current list.
/// Spawns one async task per keypress to fetch ordered IDs and pick neighbor.
fn move_selection(state: AppState, direction: i32) {
    let mut state = state;
    let section = state.section.read().clone();
    let current = state.selected.read().clone();
    let space_id = *state.space_id.read();

    match section {
        Section::AllPages => {
            spawn(async move {
                let ids = backend::current()
                    .list_page_ids_ordered(space_id, 200)
                    .await
                    .unwrap_or_default();
                navigate_ids(&ids, &current, direction, &mut state, Selected::Page);
            });
        }
        Section::AllNotes | Section::Folder(_) => {
            let folder_id = match &section {
                Section::Folder(id) => Some(*id),
                _ => None,
            };
            spawn(async move {
                let ids = backend::current()
                    .list_note_ids_ordered(folder_id, space_id)
                    .await
                    .unwrap_or_default();
                navigate_ids(&ids, &current, direction, &mut state, Selected::Note);
            });
        }
        Section::Settings => {
            state.selected.set(Selected::SettingsRules);
        }
        _ => {}
    }
}

fn navigate_ids(
    ids: &[i64],
    current: &Selected,
    direction: i32,
    state: &mut AppState,
    make_selected: impl Fn(i64) -> Selected,
) {
    if ids.is_empty() {
        return;
    }
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
