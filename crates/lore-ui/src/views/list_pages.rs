use dioxus::prelude::*;
use crate::state::{AppState, Selected};
use crate::data;
use crate::texts;

#[component]
pub fn ListPages() -> Element {
    let mut state = use_context::<AppState>();
    let mut pages = use_signal(|| data::list_pages(200).unwrap_or_default());

    // Re-fetch whenever refresh_tick changes
    // Refresh on manual bump (trash, add URL, etc.)
    let tick = state.refresh_tick;
    use_effect(move || {
        let _ = *tick.read();
        pages.set(data::list_pages(200).unwrap_or_default());
    });

    // Auto-poll every 5 seconds (worker updates in background)
    // Only poll when no item is selected to avoid disrupting navigation
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            if *state.selected.read() == crate::state::Selected::None {
                pages.set(data::list_pages(200).unwrap_or_default());
            }
        }
    });

    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", {texts::LIST_PAGES} }
            div { class: "list-items",
                if pages.read().is_empty() {
                    div { class: "empty-state", {texts::EMPTY_PAGES} }
                }
                for page in pages.read().iter() {
                    {
                        let id = page.id;
                        let is_selected = matches!(&*state.selected.read(), Selected::Page(pid) if *pid == id);
                        let cls = if is_selected { "list-item selected" } else { "list-item" };
                        rsx! {
                            div { key: "{id}", class: "{cls}",
                                onclick: move |_| {
                                    state.selected.set(Selected::Page(id));
                                },
                                div { class: "list-item-title", "{page.title}" }
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
