use crate::state::{AppState, Selected};
use crate::texts;
use dioxus::prelude::*;

#[component]
pub fn ListSettings() -> Element {
    let mut state = use_context::<AppState>();
    let selected = state.selected.read().clone();

    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", {texts::LIST_SETTINGS} }
            div { class: "list-items",
                div {
                    class: if matches!(selected, Selected::SettingsSpaces) { "list-item selected" } else { "list-item" },
                    onclick: move |_| state.selected.set(Selected::SettingsSpaces),
                    div { class: "list-item-title", {texts::SETTINGS_SPACES} }
                }
                div {
                    class: if matches!(selected, Selected::SettingsRules) { "list-item selected" } else { "list-item" },
                    onclick: move |_| state.selected.set(Selected::SettingsRules),
                    div { class: "list-item-title", {texts::SETTINGS_WEBPAGE_RULES} }
                }
            }
        }
    }
}
