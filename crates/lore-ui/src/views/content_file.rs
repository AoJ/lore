use dioxus::prelude::*;
use crate::texts;

#[component]
pub fn ContentFile(id: i64) -> Element {
    rsx! {
        div { class: "content-panel",
            div { class: "empty-state", {texts::EMPTY_FILES} }
        }
    }
}
