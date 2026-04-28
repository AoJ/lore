use dioxus::prelude::*;
use crate::state::{AppState, Section};
use crate::data;
use crate::texts;

#[component]
pub fn Sidebar() -> Element {
    let mut state = use_context::<AppState>();
    let mut url_input = use_signal(String::new);
    let mut status_msg = use_signal(|| Option::<String>::None);

    let space_id = *state.space_id.read();
    let mut spaces = use_signal(|| {
        let conn = data::open_db().unwrap();
        lore_core::db::list_spaces(&conn).unwrap_or_default()
    });
    let mut folders = use_signal(move || {
        let conn = data::open_db().unwrap();
        lore_core::db::list_folders(&conn, space_id).unwrap_or_default()
    });
    let mut trash_count = use_signal(|| lore_core::db::trash_count(&data::open_db().unwrap()).unwrap_or(0));
    let mut note_counts = use_signal(|| {
        lore_core::db::folder_note_counts(&data::open_db().unwrap(), space_id).unwrap_or_default()
    });

    let tick = state.refresh_tick;
    use_effect(move || {
        let _ = *tick.read();
        let sid = *state.space_id.read();
        let conn = data::open_db().unwrap();
        trash_count.set(lore_core::db::trash_count(&conn).unwrap_or(0));
        note_counts.set(lore_core::db::folder_note_counts(&conn, sid).unwrap_or_default());
        spaces.set(lore_core::db::list_spaces(&conn).unwrap_or_default());
        folders.set(lore_core::db::list_folders(&conn, sid).unwrap_or_default());
    });

    let on_add_url = move |evt: FormEvent| {
        evt.prevent_default();
        let raw_url = url_input.read().trim().to_string();
        if raw_url.is_empty() { return; }
        let sid = *state.space_id.read();
        match data::add_url(&raw_url, sid) {
            Ok(msg) => {
                status_msg.set(Some(msg));
                url_input.set(String::new());
                state.bump_refresh();
            }
            Err(e) => status_msg.set(Some(format!("Error: {}", e))),
        }
    };

    let section = state.section.read().clone();
    let dropdown_open = *state.space_dropdown_open.read();
    let active_space_name = spaces.read().iter()
        .find(|s| s.id == space_id)
        .map(|s| s.name.clone())
        .unwrap_or("Space".to_string());

    // Build folder tree (already filtered by space in DB query)
    let space_folders: Vec<FolderData> = folders.read().iter()
        .map(FolderData::from)
        .collect();
    let root_folders: Vec<&FolderData> = space_folders.iter()
        .filter(|f| f.parent_id.is_none())
        .collect();

    rsx! {
        nav { class: "sidebar",
            // Space switcher (replaces "lore" title)
            div { class: "space-switcher",
                div { class: "space-switcher-current",
                    onclick: move |_| {
                        let open = *state.space_dropdown_open.read();
                        state.space_dropdown_open.set(!open);
                    },
                    span { class: "space-name", "{active_space_name}" }
                    span { class: "space-arrow", if dropdown_open { "▲" } else { "▼" } }
                }
                if dropdown_open {
                    div { class: "space-dropdown",
                        for space in spaces.read().iter() {
                            {
                                let sid = space.id;
                                let is_active = sid == space_id;
                                let cls = if is_active { "space-dropdown-item active" } else { "space-dropdown-item" };
                                let is_renaming = matches!(&*state.renaming.read(), Some(crate::state::Renaming::Space(rid, _)) if *rid == sid);
                                rsx! {
                                    if is_renaming {
                                        SpaceRenameInput { space_id: sid }
                                    } else {
                                        div { key: "{sid}", class: "{cls}",
                                            onclick: move |_| state.switch_space(sid),
                                            "{space.name}"
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "space-dropdown-item new-space",
                            onclick: move |_| {
                                let conn = data::open_db().unwrap();
                                if let Ok(new_id) = lore_core::db::insert_space(&conn, "") {
                                    state.switch_space(new_id);
                                    state.renaming.set(Some(crate::state::Renaming::Space(new_id, String::new())));
                                    state.space_dropdown_open.set(true);
                                    spaces.set(lore_core::db::list_spaces(&data::open_db().unwrap()).unwrap_or_default());
                                }
                            },
                            "+ New space..."
                        }
                    }
                }
            }

            // Sections
            div { class: "sidebar-group",
                SidebarItem { label: texts::NAV_WEBS, active: section == Section::AllPages,
                    onclick: move |_| state.navigate(Section::AllPages) }
                SidebarItem { label: texts::NAV_NOTES, active: section == Section::AllNotes,
                    onclick: move |_| state.navigate(Section::AllNotes) }
                SidebarItem { label: texts::NAV_FILES, active: section == Section::AllFiles,
                    onclick: move |_| state.navigate(Section::AllFiles) }
                SidebarItem { label: texts::NAV_SEARCH, active: section == Section::Search,
                    onclick: move |_| state.navigate(Section::Search) }
            }

            // Folders
            div { class: "sidebar-divider",
                span { {texts::DIVIDER_FOLDERS} }
                span { class: "sidebar-add-btn",
                    onclick: move |_| {
                        let sid = *state.space_id.read();
                        let conn = data::open_db().unwrap();
                        if let Ok(fid) = lore_core::db::insert_folder(&conn, "", None, sid) {
                            state.renaming.set(Some(crate::state::Renaming::Folder(fid, String::new())));
                            state.bump_refresh();
                        }
                    },
                    "+"
                }
            }
            div { class: "sidebar-group",
                for folder in root_folders.iter() {
                    FolderTreeItem {
                        folder_id: folder.id,
                        name: folder.name.clone(),
                        depth: 0,
                        all_folders: space_folders.clone(),
                        note_counts: note_counts.read().clone(),
                        active_section: section.clone(),
                    }
                }
            }

            // System
            div { class: "sidebar-divider", {texts::DIVIDER_SYSTEM} }
            div { class: "sidebar-group",
                div { class: "sidebar-item{active_class(section == Section::Trash)}",
                    onclick: move |_| state.navigate(Section::Trash),
                    span { {texts::NAV_TRASH} }
                    if *trash_count.read() > 0 {
                        span { class: "badge", "{trash_count}" }
                    }
                }
                SidebarItem { label: texts::NAV_SETTINGS, active: section == Section::Settings,
                    onclick: move |_| state.navigate(Section::Settings) }
            }

            div { class: "sidebar-spacer" }

            // Add URL
            div { class: "sidebar-group",
                div { class: "sidebar-label", {texts::LABEL_ADD_URL} }
                form { class: "add-url-form", onsubmit: on_add_url,
                    input {
                        r#type: "url",
                        placeholder: texts::PLACEHOLDER_URL,
                        value: "{url_input}",
                        oninput: move |evt| url_input.set(evt.value()),
                    }
                }
                if let Some(msg) = status_msg.read().as_ref() {
                    small { class: "status-msg", "{msg}" }
                }
            }
        }
    }
}

fn active_class(active: bool) -> &'static str {
    if active { " active" } else { "" }
}

#[component]
fn SidebarItem(label: String, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let cls = if active { "sidebar-item active" } else { "sidebar-item" };
    rsx! {
        div { class: "{cls}", onclick: move |evt| onclick.call(evt),
            "{label}"
        }
    }
}

#[component]
fn SpaceRenameInput(space_id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let initial = match &*state.renaming.read() {
        Some(crate::state::Renaming::Space(_, name)) if !name.is_empty() => name.clone(),
        _ => String::new(),
    };
    let is_new = initial.is_empty();
    let mut value = use_signal(move || initial);

    rsx! {
        input {
            class: "inline-rename",
            r#type: "text",
            placeholder: "Space name...",
            value: "{value}",
            autofocus: true,
            oninput: move |evt| value.set(evt.value()),
            onkeydown: move |evt| {
                if evt.key() == Key::Enter {
                    let name = value.read().trim().to_string();
                    if !name.is_empty() {
                        let conn = data::open_db().unwrap();
                        lore_core::db::rename_space(&conn, space_id, &name).ok();
                    } else {
                        // Empty name — delete the space
                        let conn = data::open_db().unwrap();
                        lore_core::db::delete_space(&conn, space_id).ok();
                        // Switch to first remaining space
                        if let Ok(s) = lore_core::db::get_active_space(&conn) {
                            state.switch_space(s.id);
                        }
                    }
                    state.renaming.set(None);
                    state.bump_refresh();
                } else if evt.key() == Key::Escape {
                    if is_new {
                        let conn = data::open_db().unwrap();
                        lore_core::db::delete_space(&conn, space_id).ok();
                        if let Ok(s) = lore_core::db::get_active_space(&conn) {
                            state.switch_space(s.id);
                        }
                    }
                    state.renaming.set(None);
                    state.bump_refresh();
                }
            },
        }
    }
}

#[component]
fn FolderRenameInput(folder_id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let initial = match &*state.renaming.read() {
        Some(crate::state::Renaming::Folder(_, name)) if !name.is_empty() => name.clone(),
        _ => String::new(),
    };
    let is_new = initial.is_empty();
    let mut value = use_signal(move || initial);

    rsx! {
        input {
            class: "inline-rename",
            r#type: "text",
            placeholder: "Folder name...",
            value: "{value}",
            autofocus: true,
            oninput: move |evt| value.set(evt.value()),
            onkeydown: move |evt| {
                if evt.key() == Key::Enter {
                    let name = value.read().trim().to_string();
                    if !name.is_empty() {
                        let conn = data::open_db().unwrap();
                        lore_core::db::rename_folder(&conn, folder_id, &name).ok();
                    } else {
                        // Empty name — delete the folder
                        let conn = data::open_db().unwrap();
                        lore_core::db::delete_folder(&conn, folder_id).ok();
                    }
                    state.renaming.set(None);
                    state.bump_refresh();
                } else if evt.key() == Key::Escape {
                    // Cancel — delete only if new (empty name)
                    if is_new {
                        let conn = data::open_db().unwrap();
                        lore_core::db::delete_folder(&conn, folder_id).ok();
                    }
                    state.renaming.set(None);
                    state.bump_refresh();
                }
            },
        }
    }
}

#[derive(Clone, PartialEq)]
struct FolderData {
    id: i64,
    name: String,
    parent_id: Option<i64>,
    space_id: Option<i64>,
}

impl From<&lore_core::db::FolderRow> for FolderData {
    fn from(f: &lore_core::db::FolderRow) -> Self {
        Self { id: f.id, name: f.name.clone(), parent_id: f.parent_id, space_id: f.space_id }
    }
}

#[component]
fn FolderTreeItem(
    folder_id: i64,
    name: String,
    depth: usize,
    all_folders: Vec<FolderData>,
    note_counts: std::collections::HashMap<i64, i64>,
    active_section: Section,
) -> Element {
    let mut state = use_context::<AppState>();
    let mut expanded = use_signal(|| true);
    let mut menu_open = use_signal(|| false);
    let is_active = active_section == Section::Folder(folder_id);
    let children: Vec<_> = all_folders.iter().filter(|f| f.parent_id == Some(folder_id)).collect();
    let has_children = !children.is_empty();
    let count = note_counts.get(&folder_id).copied().unwrap_or(0);

    let cls = if is_active { "sidebar-item folder-item active" } else { "sidebar-item folder-item" };
    let indent = format!("{}rem", depth as f32 * 0.75);
    let is_renaming = matches!(&*state.renaming.read(), Some(crate::state::Renaming::Folder(rid, _)) if *rid == folder_id);

    rsx! {
        div { class: "{cls}", style: "padding-left: calc(var(--spacing-sm) + {indent})",
            if has_children {
                span { class: "folder-arrow",
                    onclick: move |evt| {
                        evt.stop_propagation();
                        expanded.toggle();
                    },
                    if *expanded.read() { "▾" } else { "▸" }
                }
            } else {
                span { class: "folder-arrow-placeholder" }
            }
            if is_renaming {
                FolderRenameInput { folder_id: folder_id }
            } else {
                span { class: "folder-name",
                    onclick: move |_| state.navigate(Section::Folder(folder_id)),
                    "{name}"
                }
                if count > 0 {
                    span { class: "folder-count", "{count}" }
                }
                // "..." menu button — visible on hover via CSS
                span { class: "folder-menu-btn",
                    onclick: move |evt| {
                        evt.stop_propagation();
                        menu_open.toggle();
                    },
                    "…"
                }
            }
            // Context menu dropdown
            if *menu_open.read() {
                div { class: "folder-context-menu",
                    div { class: "folder-menu-item",
                        onclick: move |_| {
                            let sid = *state.space_id.read();
                            let conn = data::open_db().unwrap();
                            if let Ok(fid) = lore_core::db::insert_folder(&conn, "", Some(folder_id), sid) {
                                state.renaming.set(Some(crate::state::Renaming::Folder(fid, String::new())));
                                expanded.set(true);
                                state.bump_refresh();
                            }
                            menu_open.set(false);
                        },
                        "New subfolder"
                    }
                    div { class: "folder-menu-item",
                        onclick: move |_| {
                            state.renaming.set(Some(crate::state::Renaming::Folder(folder_id, name.clone())));
                            menu_open.set(false);
                        },
                        "Rename"
                    }
                    div { class: "folder-menu-item danger",
                        onclick: move |_| {
                            let conn = data::open_db().unwrap();
                            lore_core::db::delete_folder(&conn, folder_id).ok();
                            if is_active {
                                state.navigate(Section::AllNotes);
                            }
                            state.bump_refresh();
                            menu_open.set(false);
                        },
                        "Delete"
                    }
                }
            }
        }
        if has_children && *expanded.read() {
            for child in children.iter() {
                FolderTreeItem {
                    key: "{child.id}",
                    folder_id: child.id,
                    name: child.name.clone(),
                    depth: depth + 1,
                    all_folders: all_folders.clone(),
                    note_counts: note_counts.clone(),
                    active_section: active_section.clone(),
                }
            }
        }
    }
}
