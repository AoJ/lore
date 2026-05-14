use crate::backend;
use crate::data;
use crate::state::{AppState, UndoAction};
use crate::store::DataStore;
use crate::texts;
use dioxus::prelude::*;

#[component]
pub fn ContentPage(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let store = use_context::<DataStore>();
    let mut page = use_signal(|| Option::<data::PageDetailView>::None);
    let mut load_error = use_signal(|| Option::<String>::None);
    let mut screenshot_expanded = use_signal(|| false);
    let space_id = *state.space_id.read();
    let mut backrefs = use_signal(Vec::<(i64, String)>::new);

    // Load page detail + back-references asynchronously.
    use_future(move || async move {
        match data::get_page_view(id).await {
            Ok(view) => {
                let url = view.url.clone();
                page.set(Some(view));
                load_error.set(None);
                backrefs.set(
                    backend::current()
                        .find_notes_referencing_url(&url, space_id)
                        .await
                        .unwrap_or_default(),
                );
            }
            Err(e) => {
                load_error.set(Some(format!("{:#}", e)));
            }
        }
    });

    if let Some(err) = load_error.read().as_ref() {
        return rsx! {
            div { class: "content-panel",
                p { class: "error", "Error: {err}" }
            }
        };
    }

    let page_read = page.read();
    let Some(p) = page_read.as_ref() else {
        return rsx! {
            div { class: "content-panel",
                div { class: "empty-state", "Loading…" }
            }
        };
    };

    rsx! {
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
                                let mut store = store;
                                let state = state;
                                spawn(async move { store.retry_page(&state, page_id).await.ok(); });
                            }
                        },
                        {texts::BTN_RETRY}
                    }
                }
                button { class: "btn btn-danger",
                    onclick: {
                        let page_id = id;
                        move |_| {
                            let mut store = store;
                            let mut state = state;
                            spawn(async move {
                                if store.trash_page(&state, page_id).await.is_ok() {
                                    state.show_toast(
                                        texts::TOAST_MOVED_TRASH.to_string(),
                                        Some(UndoAction::RestorePage(page_id)),
                                    );
                                    state.selected.set(crate::state::Selected::None);
                                }
                            });
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
            if !backrefs.read().is_empty() {
                div { class: "page-backrefs",
                    strong { "Referenced in:" }
                    for (note_id, note_title) in backrefs.read().iter() {
                        {
                            let nid = *note_id;
                            let display = if note_title.is_empty() { "Untitled note".to_string() } else { note_title.clone() };
                            rsx! {
                                span { class: "backref-link",
                                    onclick: move |_| {
                                        // Navigate to the note's folder (or root if unfiled)
                                        spawn(async move {
                                            let note_folder = backend::current()
                                                .get_note(nid)
                                                .await
                                                .ok()
                                                .and_then(|n| n.folder_id);
                                            match note_folder {
                                                Some(fid) => state.section.set(crate::state::Section::Folder(fid)),
                                                None => state.section.set(crate::state::Section::AllNotes),
                                            }
                                            state.selected.set(crate::state::Selected::Note(nid));
                                        });
                                    },
                                    "{display}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
