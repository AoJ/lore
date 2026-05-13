use dioxus::prelude::*;
use crate::state::AppState;
use crate::data;
use crate::texts;

#[component]
pub fn ListSearch() -> Element {
    let mut state = use_context::<AppState>();

    let mut page_results = use_signal(Vec::<lore_core::db::WebPageRow>::new);
    let mut note_results = use_signal(Vec::<lore_core::db::NoteRow>::new);

    let search_signal = state.search_query;
    let sid = state.space_id;
    use_effect(move || {
        let q = search_signal.read().clone();
        let s = *sid.read();
        if q.len() >= 2 {
            if let Ok(conn) = data::open_db() {
                page_results.set(
                    lore_core::search::search_web_pages_brief(&conn, &q, s, 20).unwrap_or_default(),
                );
                note_results.set(
                    lore_core::search::search_notes(&conn, &q, s, 20).unwrap_or_default(),
                );
            }
        } else {
            page_results.set(Vec::new());
            note_results.set(Vec::new());
        }
    });

    let query = state.search_query.read().clone();
    let has_results = !page_results.read().is_empty() || !note_results.read().is_empty();
    let searched = query.len() >= 2;

    rsx! {
        div { class: "list-panel",
            div { class: "list-header",
                input {
                    r#type: "search",
                    class: "search-input",
                    placeholder: texts::PLACEHOLDER_SEARCH,
                    value: "{state.search_query}",
                    oninput: move |evt| state.search_query.set(evt.value()),
                }
            }
            div { class: "list-items",
                if !searched {
                    div { class: "empty-state", {texts::EMPTY_SEARCH} }
                } else if !has_results {
                    div { class: "empty-state", {texts::empty_search_no_results(&query)} }
                }

                if !page_results.read().is_empty() {
                    div { class: "search-group-header", "{texts::SEARCH_GROUP_PAGES} ({page_results.read().len()})" }
                    for page in page_results.read().iter() {
                        {
                            let is_sel = matches!(&*state.selected.read(), crate::state::Selected::Page(pid) if *pid == page.id);
                            let cls = if is_sel { "list-item selected" } else { "list-item" };
                            let id = page.id;
                            let title = page.title.clone().unwrap_or_else(|| texts::NO_TITLE.to_string());
                            rsx! {
                                div { key: "p{page.id}", class: "{cls}",
                                    onclick: move |_| state.select_page(id),
                                    div { class: "list-item-title", "{title}" }
                                    div { class: "list-item-meta",
                                        span { "{page.domain}" }
                                    }
                                }
                            }
                        }
                    }
                }

                if !note_results.read().is_empty() {
                    div { class: "search-group-header", "{texts::SEARCH_GROUP_NOTES} ({note_results.read().len()})" }
                    for note in note_results.read().iter() {
                        {
                            let is_sel = matches!(&*state.selected.read(), crate::state::Selected::Note(nid) if *nid == note.id);
                            let cls = if is_sel { "list-item selected" } else { "list-item" };
                            let id = note.id;
                            let display_title = if note.title.is_empty() {
                                note.body_preview.lines().next().unwrap_or("Untitled").to_string()
                            } else {
                                note.title.clone()
                            };
                            rsx! {
                                div { key: "n{note.id}", class: "{cls}",
                                    onclick: move |_| state.select_note(id),
                                    div { class: "list-item-title", "{display_title}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
