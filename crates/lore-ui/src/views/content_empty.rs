use crate::texts;
use dioxus::prelude::*;

#[component]
pub fn ContentEmpty() -> Element {
    rsx! {
        div { class: "content-panel content-empty",
            div { class: "empty-state centered", {texts::EMPTY_SELECT} }
        }
    }
}
