use dioxus::prelude::*;
use crate::state::{AppState, UndoAction};
use crate::store::DataStore;
use crate::data;
use crate::texts;

#[component]
pub fn ContentPage(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();
    let page = use_signal(move || data::get_page(id));
    let mut screenshot_expanded = use_signal(|| false);

    match page.read().as_ref() {
        Ok(p) => rsx! {
            section { class: "content-panel content-page",
                h1 { class: "page-title", "{p.title}" }
                div { class: "page-url",
                    a { href: "{p.url}", target: "_blank", "{p.url}" }
                }
                div { class: "page-meta",
                    span { "{p.domain}" }
                    span { class: "sep", "·" }
                    span { "{p.category}" }
                    span { class: "sep", "·" }
                    span { "{p.status}" }
                    span { class: "sep", "·" }
                    span { "{p.created_at}" }
                    if let Some(ref size) = p.content_size {
                        span { class: "sep", "·" }
                        span { "{size}" }
                    }
                }
                if let Some(ref error) = p.last_error {
                    div { class: "page-error",
                        strong { {texts::LABEL_ERROR} }
                        span { ": {error}" }
                    }
                }
                div { class: "page-actions",
                    if p.has_snapshot {
                        button { class: "btn",
                            onclick: {
                                let url = p.url.clone();
                                move |_| data::open_in_browser(&url)
                            },
                            {texts::BTN_OPEN_BROWSER}
                        }
                    }
                    if p.status == "failed" || p.status == "queued" {
                        button { class: "btn",
                            onclick: {
                                let page_id = id;
                                move |_| {
                                    store.retry_page(&state, page_id).ok();
                                }
                            },
                            {texts::BTN_RETRY}
                        }
                    }
                    button { class: "btn btn-danger",
                        onclick: {
                            let page_id = id;
                            move |_| {
                                if store.trash_page(&state, page_id).is_ok() {
                                    state.show_toast(
                                        texts::TOAST_MOVED_TRASH.to_string(),
                                        Some(UndoAction::RestorePage(page_id)),
                                    );
                                    state.selected.set(crate::state::Selected::None);
                                }
                            }
                        },
                        {texts::BTN_DELETE}
                    }
                }
                if let Some(ref b64) = p.screenshot_base64 {
                    div {
                        class: if *screenshot_expanded.read() { "page-screenshot expanded" } else { "page-screenshot" },
                        onclick: move |_| { screenshot_expanded.toggle(); },
                        img { src: "data:image/png;base64,{b64}" }
                    }
                }
                if p.has_snapshot {
                    if let Some(ref text) = p.plain_text_preview {
                        details {
                            summary { {texts::LABEL_CONTENT_PREVIEW} }
                            pre { class: "content-preview", "{text}" }
                        }
                    }
                }
                // Back-references: which notes link to this URL
                {
                    let space_id = *state.space_id.read();
                    let refs = data::open_db().ok()
                        .and_then(|conn| lore_core::db::find_notes_referencing_url(&conn, &p.url, space_id).ok())
                        .unwrap_or_default();
                    if !refs.is_empty() {
                        rsx! {
                            div { class: "page-backrefs",
                                strong { "Referenced in:" }
                                for (note_id, note_title) in refs.iter() {
                                    {
                                        let nid = *note_id;
                                        let display = if note_title.is_empty() { "Untitled note".to_string() } else { note_title.clone() };
                                        rsx! {
                                            span { class: "backref-link",
                                                onclick: move |_| {
                                                    // Navigate to correct section (folder or root)
                                                    let note_folder = data::open_db().ok()
                                                        .and_then(|c| lore_core::db::get_note(&c, nid).ok())
                                                        .and_then(|n| n.folder_id);
                                                    match note_folder {
                                                        Some(fid) => state.section.set(crate::state::Section::Folder(fid)),
                                                        None => state.section.set(crate::state::Section::AllNotes),
                                                    }
                                                    state.selected.set(crate::state::Selected::Note(nid));
                                                },
                                                "{display}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        rsx! {}
                    }
                }
            }
        },
        Err(e) => rsx! {
            div { class: "content-panel",
                p { class: "error", "Error: {e}" }
            }
        },
    }
}
