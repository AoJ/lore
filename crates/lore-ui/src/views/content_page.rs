use crate::backend;
use crate::data::{self, VersionView};
use crate::state::{AppState, UndoAction};
use crate::store::DataStore;
use crate::texts;
use dioxus::prelude::*;

/// Which content tab is currently active. `Article` shows the
/// readability-extracted HTML in a sandboxed iframe; `Raw` shows the
/// stored plain-text preview. The selector only renders when both
/// views are available.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentTab {
    Article,
    Raw,
}

/// Detail view for an archived web page.
///
/// Data flow:
/// - One `use_effect` loads page + version list whenever `id` or
///   `refresh_tick` changes (the latter is bumped by the 2 s polling loop
///   when DB revision moves, so the panel auto-refreshes when the worker
///   appends a new snapshot or a sibling client edits).
/// - Header shows the **currently selected** version's date/time +
///   `vX` + size; click opens an inline popover listing all versions with
///   diff badges. Single-version pages show no chevron and no interaction.
/// - Title, screenshot, and plain-text preview all come from the selected
///   snapshot; switching version repaints all three.
#[component]
pub fn ContentPage(id: i64) -> Element {
    let mut state = use_context::<AppState>();
    let store = use_context::<DataStore>();

    let mut page = use_signal(|| Option::<data::PageDetailView>::None);
    let mut load_error = use_signal(|| Option::<String>::None);
    let mut screenshot_expanded = use_signal(|| false);
    let space_id = *state.space_id.read();
    let mut backrefs = use_signal(Vec::<(i64, String)>::new);

    let mut versions = use_signal(Vec::<VersionView>::new);
    // Currently displayed snapshot id. None while loading or when the page
    // has no snapshots yet; otherwise always points at one of `versions`.
    // Defaults to the newest after each reload.
    let mut selected_snapshot_id = use_signal(|| Option::<i64>::None);
    let mut selected_snapshot = use_signal(|| Option::<lore_core::db::SnapshotContent>::None);
    let mut version_picker_open = use_signal(|| false);
    let mut export_menu_open = use_signal(|| false);
    // Lazy-loaded full screenshot for the currently displayed snapshot.
    // `None` until the user clicks "expand"; cleared on snapshot change so
    // we don't keep the previous version's PNG in memory.
    let mut full_screenshot_b64 = use_signal(|| Option::<String>::None);
    // Selected content tab. Defaults to Article when readability HTML
    // exists, otherwise Raw — set in the effect that loads `selected_snapshot`
    // so the default reflects the actual snapshot, not last view.
    let mut content_tab = use_signal(|| ContentTab::Raw);

    // Reactive loader: re-runs on page id change, an explicit refresh
    // bump (delete-version, re-archive — same-tab user actions), or the
    // polling loop's revision bump (worker appended a snapshot, sibling
    // client edited). All three are signal reads inside the effect, so
    // Dioxus tracks them as dependencies.
    use_effect(move || {
        let _ = id;
        let _tick = *state.refresh_tick.read();
        let _rev = *store.revision.read();
        spawn(async move {
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
                    let metas = backend::current()
                        .list_page_versions(id)
                        .await
                        .unwrap_or_default();
                    let views: Vec<VersionView> = metas
                        .iter()
                        .map(|m| data::snapshot_meta_to_view(m, &title_fallback))
                        .collect();
                    // Default to newest (first), but preserve current
                    // selection if its id still exists (avoids snapping
                    // back to v3 every poll while user inspects v1).
                    let current = *selected_snapshot_id.read();
                    let keep_current = current
                        .map(|sid| views.iter().any(|v| v.id == sid))
                        .unwrap_or(false);
                    let new_selected = if keep_current {
                        current
                    } else {
                        views.first().map(|v| v.id)
                    };
                    versions.set(views);
                    selected_snapshot_id.set(new_selected);
                }
                Err(e) => {
                    load_error.set(Some(format!("{:#}", e)));
                }
            }
        });
    });

    // Fetch the body of whichever snapshot is selected. `None` means no
    // snapshots exist yet (queued/failed page). Also resets the lazy-loaded
    // full screenshot — without this, switching to v2 would still show v1's
    // expanded PNG until the user re-clicked.
    use_effect(move || {
        let sid = *selected_snapshot_id.read();
        full_screenshot_b64.set(None);
        screenshot_expanded.set(false);
        spawn(async move {
            match sid {
                Some(s) => {
                    if let Ok(content) = backend::current().get_page_version(s).await {
                        // Default tab follows the snapshot: Article if the
                        // worker captured one, else Raw. Per-snapshot reset
                        // keeps version switching predictable.
                        let default = if content.readability_html.is_some() {
                            ContentTab::Article
                        } else {
                            ContentTab::Raw
                        };
                        content_tab.set(default);
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

    // Resolve the "active" view-model: prefer selected snapshot's title,
    // date, size, screenshot; fall back to page-level values when no
    // snapshot is loaded yet.
    let active_snap = selected_snapshot.read();
    let versions_read = versions.read();
    let active_version_view = selected_snapshot_id
        .read()
        .and_then(|sid| versions_read.iter().find(|v| v.id == sid).cloned());

    let header_title = active_snap
        .as_ref()
        .and_then(|s| s.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| p.title.clone());

    // Default render uses the cheap thumbnail. The full screenshot is loaded
    // lazily into `full_screenshot_b64` on first click-to-enlarge — and
    // reused while the user keeps it open. Falls back to thumb-less mode
    // for snapshots without a thumb (legacy or HTTP-only) where the lazy
    // full-screenshot is the only image source.
    let preview = active_snap
        .as_ref()
        .map(|s| s.plain_text_preview.clone())
        .unwrap_or_else(|| p.plain_text_preview.clone());

    let thumb_b64: Option<String> = active_snap
        .as_ref()
        .and_then(|s| {
            s.screenshot_thumb.as_ref().map(|bytes| {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.encode(bytes)
            })
        })
        .or_else(|| p.screenshot_thumb_base64.clone());

    let has_full_for_active = active_snap
        .as_ref()
        .map(|s| s.has_full_screenshot)
        .unwrap_or(p.has_full_screenshot);

    let header_date = active_version_view
        .as_ref()
        .map(|v| v.fetched_at_display.clone())
        .or_else(|| p.last_fetched_at_display.clone());
    let header_version_label = active_version_view.as_ref().map(|v| format!("v{}", v.version));
    let header_size = p.total_size_display.clone();
    let multi_version = versions_read.len() > 1;

    rsx! {
        section { class: "content-panel content-page",
            h1 { class: "page-title", "{header_title}" }
            div { class: "page-url",
                a { href: "{p.url}", target: "_blank", "{p.url}" }
            }
            div { class: "page-meta",
                // Version selector — current vX + date. Click to open picker
                // when there's more than one version; otherwise just a label.
                if let Some(label) = header_version_label.as_ref() {
                    {
                        let is_multi = multi_version;
                        rsx! {
                            button {
                                class: if is_multi { "version-selector" } else { "version-selector single" },
                                disabled: !is_multi,
                                onclick: move |_| {
                                    if is_multi { version_picker_open.toggle(); }
                                },
                                span { class: "version-selector-num", "{label}" }
                                if let Some(d) = header_date.as_ref() {
                                    span { class: "version-selector-date", " · {d}" }
                                }
                                if is_multi {
                                    span { class: "version-selector-chevron", " ▾" }
                                }
                            }
                        }
                    }
                } else if let Some(d) = header_date.as_ref() {
                    span { "{d}" }
                }
                span { class: "sep", "·" }
                span { "{p.domain}" }
                span { class: "sep", "·" }
                span { "{p.category}" }
                span { class: "sep", "·" }
                {render_status_chip(&p.status)}
                if let Some(ref size) = header_size {
                    span { class: "sep", "·" }
                    span { title: texts::TIP_TOTAL_SIZE, "{size}" }
                }
                span { class: "sep", "·" }
                span { class: "page-id", title: texts::TIP_PAGE_ID, "#{id}" }
            }
            if *version_picker_open.read() && multi_version {
                VersionPickerPopover {
                    versions: versions_read.clone(),
                    selected_id: *selected_snapshot_id.read(),
                    on_select: move |sid: i64| {
                        selected_snapshot_id.set(Some(sid));
                        version_picker_open.set(false);
                    },
                    on_close: move |_| version_picker_open.set(false),
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
                // Retry is only meaningful when the previous fetch errored.
                // For `queued` / `fetching` the worker is going to (or
                // already is) running — nudging status back to `queued`
                // would be redundant and confuses users into thinking
                // something failed.
                if p.status == "failed" {
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
                // Export menu — only for archived snapshots (no point
                // exporting a queued/failed page with no snapshot yet).
                if p.has_snapshot {
                    {
                        // Export task is spawned from THIS scope (ContentPage),
                        // not from inside ExportMenu — the menu closes itself
                        // on selection, and Dioxus aborts tasks owned by the
                        // unmounted scope. Doing the spawn here keeps the
                        // dialog → write → toast pipeline alive past the menu
                        // close. The menu is reduced to a pure UI shell that
                        // forwards the selected format upwards.
                        let active_sid = selected_snapshot_id.read().unwrap_or(0);
                        let on_export_choice = move |fmt: lore_core::export::Format| {
                            export_menu_open.set(false);
                            if active_sid <= 0 {
                                return;
                            }
                            #[cfg(feature = "desktop")]
                            spawn(async move {
                                let mut state = state;
                                if let Ok((filename, bytes)) =
                                    backend::current().export_snapshot(active_sid, fmt).await
                                {
                                    let default_dir = dirs::download_dir().unwrap_or_default();
                                    let handle = rfd::AsyncFileDialog::new()
                                        .set_file_name(&filename)
                                        .set_directory(&default_dir)
                                        .save_file()
                                        .await;
                                    if let Some(h) = handle
                                        && h.write(&bytes).await.is_ok()
                                    {
                                        state.show_toast(
                                            texts::TOAST_EXPORTED.to_string(),
                                            None,
                                        );
                                    }
                                }
                            });
                            #[cfg(not(feature = "desktop"))]
                            let _ = fmt;
                        };
                        rsx! {
                            div { class: "export-menu-wrapper",
                                button { class: "btn",
                                    onclick: move |_| export_menu_open.toggle(),
                                    {texts::BTN_EXPORT}
                                }
                                if *export_menu_open.read() {
                                    ExportMenu {
                                        snapshot_id: active_sid,
                                        on_choose: EventHandler::new(on_export_choice),
                                    }
                                }
                            }
                        }
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
            {
                // Three render states, in order of preference:
                //   - Thumb image (cheap, default) — when snapshot has one
                //   - Full image (expanded or fallback) — when no thumb but
                //     full is already loaded into memory
                //   - Placeholder "Load full screenshot" button — when no
                //     thumb but a full exists in DB (legacy snapshot before
                //     m0010, or a future case where thumb generation failed).
                //     Without this the user sees nothing and has no way to
                //     access the screenshot.
                let expanded = *screenshot_expanded.read();
                let full = full_screenshot_b64.read().clone();
                let active_id_for_load = *selected_snapshot_id.read();

                // Click behavior is identical for the image and the
                // placeholder: toggle expanded + lazy-fetch the full PNG
                // the first time. Inlined twice (instead of a shared closure)
                // because Dioxus event handlers each need their own `move`
                // capture; a single FnMut closure can't be moved twice.
                let img_src: Option<(String, bool)> = if expanded && full.is_some() {
                    full.clone().map(|b| (b, true))
                } else if let Some(t) = thumb_b64.clone() {
                    Some((t, false))
                } else {
                    full.clone().map(|b| (b, true))
                };

                match img_src {
                    Some((src, is_full)) => rsx! {
                        div {
                            class: if expanded { "page-screenshot expanded" } else { "page-screenshot" },
                            onclick: move |_| {
                                let now_expanded = !*screenshot_expanded.read();
                                screenshot_expanded.set(now_expanded);
                                if full_screenshot_b64.read().is_none()
                                    && has_full_for_active
                                    && let Some(sid) = active_id_for_load
                                {
                                    spawn(async move {
                                        if let Ok(Some(bytes)) =
                                            backend::current().get_snapshot_full_screenshot(sid).await
                                        {
                                            use base64::Engine;
                                            full_screenshot_b64.set(Some(
                                                base64::engine::general_purpose::STANDARD.encode(&bytes),
                                            ));
                                        }
                                    });
                                }
                            },
                            img {
                                src: "data:image/png;base64,{src}",
                                class: if is_full { "page-screenshot-img full" } else { "page-screenshot-img thumb" },
                            }
                        }
                    },
                    None if has_full_for_active => rsx! {
                        // Snapshot has a full screenshot in DB but no thumb
                        // (legacy pre-m0010 row, or a future case where
                        // thumb generation failed). Without this affordance
                        // the user sees nothing.
                        button {
                            class: "screenshot-placeholder",
                            onclick: move |_| {
                                let now_expanded = !*screenshot_expanded.read();
                                screenshot_expanded.set(now_expanded);
                                if full_screenshot_b64.read().is_none()
                                    && let Some(sid) = active_id_for_load
                                {
                                    spawn(async move {
                                        if let Ok(Some(bytes)) =
                                            backend::current().get_snapshot_full_screenshot(sid).await
                                        {
                                            use base64::Engine;
                                            full_screenshot_b64.set(Some(
                                                base64::engine::general_purpose::STANDARD.encode(&bytes),
                                            ));
                                        }
                                    });
                                }
                            },
                            span { class: "screenshot-placeholder-icon", "🖼" }
                            span { class: "screenshot-placeholder-label",
                                {texts::BTN_LOAD_SCREENSHOT}
                            }
                        }
                    },
                    None => rsx! {},
                }
            }
            if p.has_snapshot {
                {
                    // Article HTML comes from the freshly loaded snapshot
                    // (preferred) or, while that's still in flight, the
                    // page-level snapshot loaded with `get_page` so the
                    // first paint already shows the cleaned article.
                    let article_html = active_snap
                        .as_ref()
                        .and_then(|s| s.readability_html.clone())
                        .or_else(|| p.readability_html.clone());
                    let tab = *content_tab.read();
                    let has_article = article_html.is_some();
                    rsx! {
                        // Only show tabs when both views exist — single-view
                        // pages don't need the chrome.
                        if has_article {
                            div { class: "content-tabs",
                                button {
                                    class: if tab == ContentTab::Article { "content-tab active" } else { "content-tab" },
                                    onclick: move |_| content_tab.set(ContentTab::Article),
                                    {texts::TAB_ARTICLE}
                                }
                                button {
                                    class: if tab == ContentTab::Raw { "content-tab active" } else { "content-tab" },
                                    onclick: move |_| content_tab.set(ContentTab::Raw),
                                    {texts::TAB_RAW}
                                }
                            }
                        }
                        match (tab, article_html, preview.clone()) {
                            (ContentTab::Article, Some(html), _) => {
                                // Web (Blink): renders correctly.
                                // Desktop (WKWebView via dioxus:// custom
                                // scheme): renders BLANK — see PLAN.md
                                // "Web archivace — pokročilé" → tech debt
                                // for the things we tried. Falling back to
                                // srcdoc + manual escape keeps the web
                                // build functional; desktop users see an
                                // empty iframe and have to use the Raw tab
                                // or the HTML export to read the article.
                                let escaped =
                                    html.replace('&', "&amp;").replace('"', "&quot;");
                                rsx! {
                                    iframe {
                                        class: "content-article",
                                        srcdoc: "{escaped}",
                                    }
                                }
                            }
                            (_, _, Some(text)) => rsx! {
                                pre { class: "content-preview", "{text}" }
                            },
                            _ => rsx! {},
                        }
                    }
                }
            }
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
fn VersionPickerPopover(
    versions: Vec<VersionView>,
    selected_id: Option<i64>,
    on_select: EventHandler<i64>,
    on_close: EventHandler<()>,
) -> Element {
    let state = use_context::<AppState>();
    let store = use_context::<DataStore>();
    let multiple = versions.len() > 1;
    let latest_id = versions.first().map(|v| v.id);

    rsx! {
        div { class: "version-picker-popover",
            div { class: "version-picker-header",
                span { {texts::LABEL_VERSIONS} }
                span { class: "page-versions-count", " ({versions.len()})" }
                button {
                    class: "btn-icon version-picker-close",
                    onclick: move |_| on_close.call(()),
                    "×"
                }
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
                                onclick: move |_| on_select.call(vid),
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
                                            spawn(async move {
                                                if store.delete_page_version(&mut state2, vid).await.is_ok() {
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

/// Pure UI shell for the export popover.
///
/// Desktop branch fires `on_choose` and lets the parent spawn the actual
/// `backend.export_snapshot` + save-dialog task. That separation is
/// load-bearing: Dioxus aborts tasks owned by the unmounted ExportMenu
/// scope, so a self-contained `spawn` here would die the moment the
/// popover closes.
///
/// Web branch is a plain `<a download>` pointing at the raw GET endpoint —
/// the browser handles the save dialog natively, no Rust task needed.
#[component]
fn ExportMenu(snapshot_id: i64, on_choose: EventHandler<lore_core::export::Format>) -> Element {
    let formats = [
        (lore_core::export::Format::Html, texts::EXPORT_HTML),
        (lore_core::export::Format::Markdown, texts::EXPORT_MARKDOWN),
        (lore_core::export::Format::Json, texts::EXPORT_JSON),
    ];

    rsx! {
        div { class: "export-menu",
            for (fmt, label) in formats.iter().copied() {
                {
                    #[cfg(feature = "desktop")]
                    {
                        rsx! {
                            button {
                                key: "{label}",
                                class: "export-menu-item",
                                onclick: move |_| on_choose.call(fmt),
                                "{label}"
                            }
                        }
                    }
                    #[cfg(not(feature = "desktop"))]
                    {
                        let fmt_str = match fmt {
                            lore_core::export::Format::Html => "html",
                            lore_core::export::Format::Markdown => "markdown",
                            lore_core::export::Format::Json => "json",
                        };
                        let href = format!(
                            "/api/snapshots/{}/export?format={}",
                            snapshot_id, fmt_str
                        );
                        rsx! {
                            a {
                                key: "{label}",
                                class: "export-menu-item",
                                href: "{href}",
                                onclick: move |_| on_choose.call(fmt),
                                "{label}"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Render the page status as a colored chip so `queued` / `fetching` /
/// `failed` are distinguishable at a glance — previously they were all
/// plain text and a queued page looked indistinguishable from a failed one.
fn render_status_chip(status: &str) -> Element {
    let (class, text) = match status {
        "queued" => ("status-chip status-queued", texts::STATUS_QUEUED),
        "fetching" => ("status-chip status-fetching", texts::STATUS_FETCHING),
        "archived" => ("status-chip status-archived", texts::STATUS_ARCHIVED),
        "failed" => ("status-chip status-failed", texts::STATUS_FAILED),
        "skipped" => ("status-chip status-skipped", texts::STATUS_SKIPPED),
        // Unknown statuses fall through with the raw text so we don't
        // accidentally hide new DB values from view.
        other => ("status-chip", other),
    };
    rsx! { span { class: "{class}", "{text}" } }
}
