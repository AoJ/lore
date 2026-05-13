use dioxus::prelude::*;
use crate::state::{AppState, Selected};
use crate::store::DataStore;
use crate::texts;

#[component]
pub fn ListPages() -> Element {
    let mut state = use_context::<AppState>();
    let store = use_context::<DataStore>();

    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", {texts::LIST_PAGES} }
            div { class: "list-items",
                if store.pages.read().is_empty() {
                    div { class: "empty-state", {texts::EMPTY_PAGES} }
                }
                for page in store.pages.read().iter() {
                    {
                        let id = page.id;
                        let is_selected = matches!(&*state.selected.read(), Selected::Page(pid) if *pid == id);
                        let cls = if is_selected { "list-item selected" } else { "list-item" };
                        let title = page.title.clone().unwrap_or_else(|| texts::NO_TITLE.to_string());
                        rsx! {
                            div { key: "{id}", class: "{cls}",
                                onclick: move |_| state.selected.set(Selected::Page(id)),
                                div { class: "list-item-title", "{title}" }
                                div { class: "list-item-meta",
                                    span { "{page.domain}" }
                                    span { class: "sep", "·" }
                                    span { "{page.status}" }
                                }
                                div { class: "list-item-date", "{page.created_at}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
