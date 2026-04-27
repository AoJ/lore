use dioxus::prelude::*;
use crate::state::{AppState, Selected};
use crate::texts;

#[component]
pub fn ListSettings() -> Element {
    let mut state = use_context::<AppState>();
    let selected = state.selected.read().clone();
    let is_rules = matches!(selected, Selected::SettingsRules);

    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", {texts::LIST_SETTINGS} }
            div { class: "list-items",
                div {
                    class: if is_rules { "list-item selected" } else { "list-item" },
                    onclick: move |_| state.selected.set(Selected::SettingsRules),
                    div { class: "list-item-title", {texts::SETTINGS_WEBPAGE_RULES} }
                }
            }
        }
    }
}
