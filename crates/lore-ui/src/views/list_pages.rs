use dioxus::prelude::*;
use crate::state::{AppState, Selected};
use crate::data;
use crate::texts;

#[component]
pub fn ListPages() -> Element {
    let mut state = use_context::<AppState>();
    let space_id = *state.space_id.read();
    let mut pages = use_signal(move || data::list_pages(space_id, 200).unwrap_or_default());

    let tick = state.refresh_tick;
    let sid = state.space_id;
    use_effect(move || {
        let _ = *tick.read();
        let s = *sid.read();
        pages.set(data::list_pages(s, 200).unwrap_or_default());
    });

    // Revision-based polling — only re-fetch when DB actually changed
    use_future(move || async move {
        let mut last_rev = data::get_revision();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let current_rev = data::get_revision();
            if current_rev != last_rev {
                last_rev = current_rev;
                let s = *state.space_id.read();
                pages.set(data::list_pages(s, 200).unwrap_or_default());
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
