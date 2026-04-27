use dioxus::prelude::*;
use crate::texts;

#[component]
pub fn ListFiles() -> Element {
    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", {texts::LIST_FILES} }
            div { class: "list-items",
                div { class: "empty-state", {texts::EMPTY_FILES} }
            }
        }
    }
}
