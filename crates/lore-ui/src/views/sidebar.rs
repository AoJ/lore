use dioxus::prelude::*;
use crate::state::{AppState, Section};
use crate::data;
use crate::texts;

#[component]
pub fn Sidebar() -> Element {
    let mut state = use_context::<AppState>();
    let mut url_input = use_signal(String::new);
    let mut status_msg = use_signal(|| Option::<String>::None);
    let folders = use_signal(|| lore_core::db::list_folders(&data::open_db().unwrap()).unwrap_or_default());
    let mut trash_count = use_signal(|| lore_core::db::trash_count(&data::open_db().unwrap()).unwrap_or(0));

    let tick = state.refresh_tick;
    use_effect(move || {
        let _ = *tick.read();
        trash_count.set(lore_core::db::trash_count(&data::open_db().unwrap()).unwrap_or(0));
    });

    let on_add_url = move |evt: FormEvent| {
        evt.prevent_default();
        let raw_url = url_input.read().trim().to_string();
        if raw_url.is_empty() {
            return;
        }
        match data::add_url(&raw_url) {
            Ok(msg) => {
                status_msg.set(Some(msg));
                url_input.set(String::new());
                state.bump_refresh();
            }
            Err(e) => {
                status_msg.set(Some(format!("Error: {}", e)));
            }
        }
    };

    let section = state.section.read().clone();

    rsx! {
        nav { class: "sidebar",
            h1 { class: "sidebar-title", {texts::APP_TITLE} }

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

            if !folders.read().is_empty() {
                div { class: "sidebar-divider", {texts::DIVIDER_FOLDERS} }
                div { class: "sidebar-group",
                    for folder in folders.read().iter() {
                        SidebarItem {
                            key: "{folder.id}",
                            label: "{folder.name}",
                            active: section == Section::Folder(folder.id),
                            onclick: {
                                let fid = folder.id;
                                move |_| state.navigate(Section::Folder(fid))
                            }
                        }
                    }
                }
            }

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
