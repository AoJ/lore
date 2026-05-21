use crate::backend;
use crate::data::{self, VersionView};
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

    // Version list + selection. `selected_snapshot_id = None` means "show the
    // latest" — what `get_page` already returns. Selecting an older version
    // re-fetches `get_page_version(snapshot_id)` and overrides the rendered
    // preview/screenshot.
    let mut versions = use_signal(Vec::<VersionView>::new);
    let mut selected_snapshot_id = use_signal(|| Option::<i64>::None);
    let mut selected_snapshot = use_signal(|| Option::<lore_core::db::SnapshotContent>::None);

    // Re-run on refresh tick bumps (re-archive, delete-version) so the panel
    // reflects fresh DB state without a manual reload.
    let refresh_tick = *state.refresh_tick.read();

    use_future(move || async move {
        let _tick = refresh_tick;
        match data::get_page_view(id).await {
            Ok(view) => {
                let url = view.url.clone();
                let title_fallback = view.title.clone();
                page.set(Some(view));
                load_error.set(None);
                backrefs.set(
                    backend::current()
                        .find_notes_referencing_url(&url, space_id)
                        .await
                        .unwrap_or_default(),
                );
                // Refresh versions panel
                let metas = backend::current()
                    .list_page_versions(id)
                    .await
                    .unwrap_or_default();
                versions.set(
                    metas
                        .iter()
                        .map(|m| data::snapshot_meta_to_view(m, &title_fallback))
                        .collect(),
                );
            }
            Err(e) => {
                load_error.set(Some(format!("{:#}", e)));
            }
        }
    });

    // When user picks a non-latest version, fetch its body.
    use_effect(move || {
        let sid = *selected_snapshot_id.read();
        spawn(async move {
            match sid {
                Some(s) => {
                    if let Ok(content) = backend::current().get_page_version(s).await {
                        selected_snapshot.set(Some(content));
                    }
                }
                None => selected_snapshot.set(None),
            }
        });
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

    // Resolve which snapshot to render: an explicitly selected old version,
    // or fall back to the latest data already in `PageDetailView`.
    let active_snapshot = selected_snapshot.read();
    let (size_display, preview, screenshot_b64): (Option<String>, Option<String>, Option<String>) =
        match active_snapshot.as_ref() {
            Some(s) => {
                let b64 = s.screenshot.as_ref().map(|bytes| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                });
                (
                    Some(data::format_size_short_pub(s.size_bytes)),
                    s.plain_text_preview.clone(),
                    b64,
                )
            }
            None => (
                p.content_size.clone(),
                p.plain_text_preview.clone(),
                p.screenshot_base64.clone(),
            ),
        };
    let viewing_old_version = active_snapshot.is_some();

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
                if let Some(ref size) = size_display {
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
                    {
                        #[cfg(feature = "desktop")]
                        {
                            let url = p.url.clone();
                            rsx! {
                                button { class: "btn",
                                    onclick: move |_| data::open_in_browser(&url),
                                    {texts::BTN_OPEN_BROWSER}
                                }
                            }
                        }
                        #[cfg(not(feature = "desktop"))]
                        {
                            rsx! {
                                a { class: "btn", href: "{p.url}", target: "_blank",
                                    {texts::BTN_OPEN_BROWSER}
                                }
                            }
                        }
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
                if p.has_snapshot {
                    button { class: "btn",
                        onclick: {
                            let page_id = id;
                            move |_| {
                                let mut store = store;
                                let mut state = state;
                                spawn(async move {
                                    if store.reachive_page(&state, page_id).await.is_ok() {
                                        state.show_toast(texts::TOAST_REACHIVE_QUEUED.to_string(), None);
                                    }
                                });
                            }
                        },
                        {texts::BTN_REACHIVE}
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
            if let Some(ref b64) = screenshot_b64 {
                div {
                    class: if *screenshot_expanded.read() { "page-screenshot expanded" } else { "page-screenshot" },
                    onclick: move |_| { screenshot_expanded.toggle(); },
                    img { src: "data:image/png;base64,{b64}" }
                }
            }
            if p.has_snapshot {
                if let Some(ref text) = preview {
                    details {
                        open: viewing_old_version,
                        summary { {texts::LABEL_CONTENT_PREVIEW} }
                        pre { class: "content-preview", "{text}" }
                    }
                }
            }
            // ---- Versions list ----
            // Only render once we have at least one version. Single-version
            // pages still show the panel so users learn the concept.
            if !versions.read().is_empty() {
                VersionsPanel {
                    page_id: id,
                    versions: versions.read().clone(),
                    selected_id: *selected_snapshot_id.read(),
                    on_select: move |sid: Option<i64>| selected_snapshot_id.set(sid),
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

#[component]
fn VersionsPanel(
    page_id: i64,
    versions: Vec<VersionView>,
    selected_id: Option<i64>,
    on_select: EventHandler<Option<i64>>,
) -> Element {
    let _ = page_id; // Reserved for future per-page actions (e.g. compare).
    let state = use_context::<AppState>();
    let store = use_context::<DataStore>();
    let multiple = versions.len() > 1;
    // Latest is first in the list (DB returns DESC by version).
    let latest_id = versions.first().map(|v| v.id);

    rsx! {
        details { class: "page-versions", open: true,
            summary {
                span { {texts::LABEL_VERSIONS} }
                span { class: "page-versions-count", " ({versions.len()})" }
            }
            ul { class: "version-list",
                for v in versions.iter() {
                    {
                        let vid = v.id;
                        let is_latest = Some(vid) == latest_id;
                        let is_selected = match selected_id {
                            Some(s) => s == vid,
                            None => is_latest,
                        };
                        let summary_badges = version_badges(&v.summary, is_latest);
                        rsx! {
                            li {
                                key: "{vid}",
                                class: if is_selected { "version-row selected" } else { "version-row" },
                                onclick: move |_| {
                                    // Clicking the latest row clears the override so the
                                    // panel goes back to `get_page` defaults.
                                    if is_latest { on_select.call(None); } else { on_select.call(Some(vid)); }
                                },
                                span { class: "version-num", "v{v.version}" }
                                span { class: "version-date", "{v.fetched_at_display}" }
                                span { class: "version-size", "{v.size_display}" }
                                span { class: "version-badges", {summary_badges} }
                                if multiple {
                                    button {
                                        class: "btn-icon btn-danger version-delete",
                                        title: texts::BTN_DELETE_VERSION,
                                        onclick: move |evt| {
                                            evt.stop_propagation();
                                            let mut store = store;
                                            let mut state2 = state;
                                            let was_selected = selected_id == Some(vid);
                                            spawn(async move {
                                                // No native confirm() in Dioxus desktop — rely on
                                                // toast Undo flow once we add it. For now, just delete.
                                                if store.delete_page_version(&mut state2, vid).await.is_ok() {
                                                    if was_selected { /* selection cleared by re-render */ }
                                                    state2.show_toast(
                                                        texts::TOAST_VERSION_DELETED.to_string(),
                                                        None,
                                                    );
                                                }
                                            });
                                        },
                                        "×"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Renders the `change_summary` flags as small inline badges.
fn version_badges(summary: &Option<data::ChangeSummary>, is_latest: bool) -> Element {
    let badges = match summary {
        None => {
            // v1 has no diff base. Latest gets a "current" tag; older v1s
            // (shouldn't happen — v1 is always oldest) show nothing.
            if is_latest {
                rsx! { span { class: "version-badge badge-current", {texts::BADGE_CURRENT} } }
            } else {
                rsx! {}
            }
        }
        Some(s) => {
            let size_badge = if s.size_delta_pct != 0 {
                let sign = if s.size_delta_pct > 0 { "+" } else { "" };
                Some(format!("{}{}%", sign, s.size_delta_pct))
            } else {
                None
            };
            rsx! {
                if is_latest {
                    span { class: "version-badge badge-current", {texts::BADGE_CURRENT} }
                }
                if s.content_same {
                    span { class: "version-badge badge-nochange", {texts::BADGE_NO_CHANGE} }
                }
                if s.title_changed {
                    span { class: "version-badge badge-title", {texts::BADGE_TITLE_CHANGED} }
                }
                if let Some(sb) = size_badge {
                    span { class: "version-badge badge-size", "{sb}" }
                }
            }
        }
    };
    rsx! { {badges} }
}
