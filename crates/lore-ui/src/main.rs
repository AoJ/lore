use dioxus::prelude::*;

const APP_CSS: &str = include_str!("../assets/app.css");

fn main() {
    use dioxus::desktop::{Config, WindowBuilder};

    let config = Config::new().with_window(
        WindowBuilder::new()
            .with_title("lore")
            .with_always_on_top(false)
            .with_inner_size(dioxus::desktop::LogicalSize::new(1200.0, 800.0)),
    );

    LaunchBuilder::desktop().with_cfg(config).launch(app);
}

fn app() -> Element {
    rsx! {
        document::Style { {APP_CSS} }
        Router::<Route> {}
    }
}

#[derive(Clone, Routable, Debug, PartialEq)]
enum Route {
    #[layout(Layout)]
    #[route("/")]
    PageList,
    #[route("/page/:id")]
    PageDetail { id: i64 },
    #[route("/rules")]
    Rules,
}

#[component]
fn Layout() -> Element {
    rsx! {
        div { class: "app-layout",
            nav { class: "app-sidebar",
                h1 { class: "app-title", "lore" }
                ul { class: "app-nav",
                    li {
                        Link { to: Route::PageList, "Pages" }
                    }
                    li {
                        Link { to: Route::Rules, "Rules" }
                    }
                }
                div { class: "sidebar-section",
                    SearchInput {}
                }
                div { class: "sidebar-spacer" }
                div { class: "sidebar-section",
                    div { class: "sidebar-section-label", "Add URL" }
                    AddUrlInput {}
                }
            }
            main { class: "app-main",
                Outlet::<Route> {}
            }
        }
    }
}

#[component]
fn AddUrlInput() -> Element {
    let mut url_input = use_signal(String::new);
    let mut status_msg = use_signal(|| Option::<String>::None);

    let on_submit = move |evt: FormEvent| {
        evt.prevent_default();
        let raw_url = url_input.read().trim().to_string();
        if raw_url.is_empty() {
            return;
        }

        match add_url_to_db(&raw_url) {
            Ok(msg) => {
                status_msg.set(Some(msg));
                url_input.set(String::new());
            }
            Err(e) => {
                status_msg.set(Some(format!("Error: {}", e)));
            }
        }
    };

    rsx! {
        form { class: "add-url-form", onsubmit: on_submit,
            input {
                r#type: "url",
                placeholder: "Add URL...",
                value: "{url_input}",
                oninput: move |evt| url_input.set(evt.value()),
            }
        }
        if let Some(msg) = status_msg.read().as_ref() {
            small { class: "status-msg", "{msg}" }
        }
    }
}

#[component]
fn SearchInput() -> Element {
    let mut query = use_signal(String::new);
    let mut results = use_signal(Vec::<PageRow>::new);

    let on_input = move |evt: FormEvent| {
        let q = evt.value();
        query.set(q.clone());
        if q.len() >= 2 {
            if let Ok(rows) = search_pages(&q, 20) {
                results.set(rows);
            }
        } else {
            results.set(Vec::new());
        }
    };

    rsx! {
        input {
            r#type: "search",
            placeholder: "Search...",
            value: "{query}",
            oninput: on_input,
        }
        if !results.read().is_empty() {
            ul { class: "search-results",
                for row in results.read().iter() {
                    li { key: "{row.id}",
                        Link { to: Route::PageDetail { id: row.id },
                            "{row.title}"
                        }
                        small { " ({row.domain})" }
                    }
                }
            }
        }
    }
}

#[component]
fn PageList() -> Element {
    let pages = use_signal(|| list_pages(None, None, None, 100).unwrap_or_default());

    rsx! {
        section { class: "page-list",
            h2 { "Pages" }
            div { class: "page-items",
                for page in pages.read().iter() {
                    Link { key: "{page.id}", class: "page-item", to: Route::PageDetail { id: page.id },
                        div { class: "page-item-title", "{page.title}" }
                        div { class: "page-item-meta",
                            span { "{page.domain}" }
                            span { class: "separator", "·" }
                            span { "{page.category}" }
                            span { class: "separator", "·" }
                            span { "{page.status}" }
                            span { class: "separator", "·" }
                            span { "{page.created_at}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn PageDetail(id: i64) -> Element {
    let page = use_signal(move || get_page(id));
    let mut screenshot_expanded = use_signal(|| false);

    match page.read().as_ref() {
        Ok(p) => rsx! {
            section { class: "page-detail",
                nav { class: "breadcrumb",
                    Link { to: Route::PageList, "Pages" }
                    span { class: "separator", "›" }
                    span { class: "current", "{p.title}" }
                }
                div { class: "page-detail-url",
                    a { href: "{p.url}", target: "_blank", "{p.url}" }
                }
                div { class: "page-detail-meta",
                    span { "{p.domain}" }
                    span { class: "separator", "·" }
                    span { "{p.category}" }
                    span { class: "separator", "·" }
                    span { "{p.status}" }
                    span { class: "separator", "·" }
                    span { "{p.created_at}" }
                    if let Some(ref size) = p.content_size {
                        span { class: "separator", "·" }
                        span { "{size}" }
                    }
                }
                div { class: "page-actions",
                    if p.has_snapshot {
                        button {
                            onclick: {
                                let url = p.url.clone();
                                move |_| open_in_browser(&url)
                            },
                            "Open in browser"
                        }
                    }
                    button { class: "btn-danger",
                        onclick: move |_| {
                            if let Ok(()) = delete_page_from_db(id) {
                                navigator().push(Route::PageList);
                            }
                        },
                        "Delete"
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
                        details { open: false,
                            summary { "Content preview" }
                            pre { class: "content-preview", "{text}" }
                        }
                    }
                }
            }
        },
        Err(e) => rsx! { p { "Error: {e}" } },
    }
}

#[component]
fn Rules() -> Element {
    let rules = use_signal(|| load_rules().unwrap_or_default());

    rsx! {
        section { class: "rules",
            h2 { "Classification Rules" }
            table { role: "grid",
                thead {
                    tr {
                        th { "Pattern" }
                        th { "Match type" }
                        th { "Category" }
                        th { "Note" }
                    }
                }
                tbody {
                    for rule in rules.read().iter() {
                        tr {
                            td { "{rule.pattern}" }
                            td { "{rule.match_type}" }
                            td { "{rule.category}" }
                            td { "{rule.note}" }
                        }
                    }
                }
            }
        }
    }
}

// --- Data layer ---

fn db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("LORE_DB") {
        return std::path::PathBuf::from(p);
    }
    if let Some(dir) = dirs::data_local_dir() {
        let p = dir.join("lore");
        std::fs::create_dir_all(&p).ok();
        return p.join("lore.db");
    }
    std::path::PathBuf::from("lore.db")
}

fn open_db() -> anyhow::Result<rusqlite::Connection> {
    lore_core::db::open(&db_path())
}

#[derive(Clone, Debug)]
struct PageRow {
    id: i64,
    title: String,
    domain: String,
    category: String,
    status: String,
    created_at: String,
}

#[derive(Clone, Debug)]
struct PageDetailData {
    url: String,
    title: String,
    domain: String,
    category: String,
    status: String,
    created_at: String,
    content_size: Option<String>,
    has_snapshot: bool,
    plain_text_preview: Option<String>,
    screenshot_base64: Option<String>,
}

#[derive(Clone, Debug)]
struct RuleRow {
    pattern: String,
    match_type: String,
    category: String,
    note: String,
}

fn add_url_to_db(raw_url: &str) -> anyhow::Result<String> {
    let conn = open_db()?;
    let rules = lore_core::db::load_rules(&conn)?;
    let parsed = url::Url::parse(raw_url)?;
    let normalized = lore_core::rules::normalize_url(&parsed);
    let domain = parsed.host_str().unwrap_or("unknown").to_string();
    let category = lore_core::rules::classify(&parsed, &rules);
    let status = if category == "archive" {
        "queued"
    } else {
        "skipped"
    };

    lore_core::db::insert_web_page(
        &conn,
        &lore_core::db::NewWebPage {
            url: raw_url,
            url_normalized: &normalized,
            title: None,
            domain: &domain,
            category: &category,
            status,
            source: None,
        },
    )?;
    Ok(format!("[{}] {}", category, raw_url))
}

fn search_pages(query: &str, limit: usize) -> anyhow::Result<Vec<PageRow>> {
    let conn = open_db()?;
    let query = if query.contains('*') || query.contains('"') || query.contains(" AND ") {
        query.to_string()
    } else {
        query
            .split_whitespace()
            .map(|w| format!("{}*", w))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut stmt = conn.prepare(
        "SELECT wp.id, wp.url, wp.title, wp.domain, wp.category
         FROM web_page_fts fts
         JOIN web_page_snapshot wps ON wps.id = fts.rowid
         JOIN web_page wp ON wp.id = wps.web_page_id
         WHERE web_page_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![query, limit as i64], |row| {
            Ok(PageRow {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                domain: row.get(3)?,
                category: row.get(4)?,
                status: String::new(),
                created_at: String::new(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn list_pages(
    category: Option<&str>,
    status: Option<&str>,
    domain: Option<&str>,
    limit: usize,
) -> anyhow::Result<Vec<PageRow>> {
    let conn = open_db()?;
    let mut sql = String::from(
        "SELECT id, url, title, domain, category, status, created_at FROM web_page WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(cat) = category {
        sql.push_str(&format!(" AND category = ?{}", idx));
        params.push(Box::new(cat.to_string()));
        idx += 1;
    }
    if let Some(st) = status {
        sql.push_str(&format!(" AND status = ?{}", idx));
        params.push(Box::new(st.to_string()));
        idx += 1;
    }
    if let Some(dom) = domain {
        sql.push_str(&format!(" AND domain LIKE ?{}", idx));
        params.push(Box::new(format!("%{}%", dom)));
        idx += 1;
    }
    sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", idx));
    params.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(PageRow {
                id: row.get(0)?,
                title: row
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "(no title)".to_string()),
                domain: row.get(3)?,
                category: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get::<_, String>(6)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn get_page(id: i64) -> anyhow::Result<PageDetailData> {
    let conn = open_db()?;

    let (url, title, domain, category, status, created_at) = conn.query_row(
        "SELECT url, title, domain, category, status, created_at FROM web_page WHERE id = ?1",
        [id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )?;

    let snapshot: Option<(String, Option<String>, Option<Vec<u8>>)> = conn
        .query_row(
            "SELECT LENGTH(html_content), SUBSTR(plain_text, 1, 2000), screenshot FROM web_page_snapshot WHERE web_page_id = ?1 ORDER BY version DESC LIMIT 1",
            [id],
            |row| {
                let size: i64 = row.get(0)?;
                let size_str = if size > 1_000_000 {
                    format!("{:.1} MB", size as f64 / 1_000_000.0)
                } else if size > 1_000 {
                    format!("{:.1} KB", size as f64 / 1_000.0)
                } else {
                    format!("{} B", size)
                };
                let screenshot: Option<Vec<u8>> = row.get(2)?;
                Ok((size_str, row.get(1)?, screenshot))
            },
        )
        .ok();

    let screenshot_base64 = snapshot
        .as_ref()
        .and_then(|(_, _, s)| s.as_ref())
        .map(|bytes| {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        });

    Ok(PageDetailData {
        url,
        title: title.unwrap_or_else(|| "(no title)".to_string()),
        domain,
        category,
        status,
        created_at: created_at.chars().take(10).collect(),
        content_size: snapshot.as_ref().map(|(s, _, _)| s.clone()),
        has_snapshot: snapshot.is_some(),
        plain_text_preview: snapshot.and_then(|(_, t, _)| t),
        screenshot_base64,
    })
}

fn load_rules() -> anyhow::Result<Vec<RuleRow>> {
    let conn = open_db()?;
    let mut stmt = conn.prepare(
        "SELECT pattern, match_type, category, COALESCE(note, '') FROM classification_rule ORDER BY priority DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(RuleRow {
                pattern: row.get(0)?,
                match_type: row.get(1)?,
                category: row.get(2)?,
                note: row.get(3)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

fn delete_page_from_db(page_id: i64) -> anyhow::Result<()> {
    let conn = open_db()?;
    lore_core::db::delete_page(&conn, page_id)?;
    Ok(())
}

fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
