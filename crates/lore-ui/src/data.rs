use anyhow::Result;
use std::path::PathBuf;

/// Resolve database path from environment or system default.
pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("LORE_DB") {
        return PathBuf::from(p);
    }
    if let Some(dir) = dirs::data_local_dir() {
        let p = dir.join("lore");
        std::fs::create_dir_all(&p).ok();
        return p.join("lore.db");
    }
    PathBuf::from("lore.db")
}

/// Open a connection for runtime queries. Skips migration runner / seed —
/// `main::app()` calls `lore_core::db::open` once at startup to bootstrap.
pub fn open_db() -> Result<rusqlite::Connection> {
    lore_core::db::open_existing(&db_path())
}

// ---- Page types & queries ----

#[derive(Clone, Debug)]
pub struct PageRow {
    pub id: i64,
    pub title: String,
    pub domain: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug)]
pub struct PageDetailData {
    pub url: String,
    pub title: String,
    pub domain: String,
    pub category: String,
    pub status: String,
    pub created_at: String,
    pub content_size: Option<String>,
    pub has_snapshot: bool,
    pub plain_text_preview: Option<String>,
    pub screenshot_base64: Option<String>,
    pub last_error: Option<String>,
}

pub fn list_pages(space_id: i64, limit: usize) -> Result<Vec<PageRow>> {
    let conn = open_db()?;
    let mut stmt = conn.prepare(
        "SELECT id, url, title, domain, status, created_at
         FROM web_page WHERE trashed_at IS NULL AND space_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![space_id, limit as i64], |row| {
            Ok(PageRow {
                id: row.get(0)?,
                title: row
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| crate::texts::NO_TITLE.to_string()),
                domain: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_page(id: i64) -> Result<PageDetailData> {
    let conn = open_db()?;

    let (url, title, domain, category, status, created_at, last_error) = conn.query_row(
        "SELECT url, title, domain, category, status, created_at, last_error FROM web_page WHERE id = ?1",
        [id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
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
                Ok((size_str, row.get(1)?, row.get(2)?))
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
        title: title.unwrap_or_else(|| crate::texts::NO_TITLE.to_string()),
        domain,
        category,
        status,
        created_at: created_at.chars().take(10).collect(),
        content_size: snapshot.as_ref().map(|(s, _, _)| s.clone()),
        has_snapshot: snapshot.is_some(),
        plain_text_preview: snapshot.and_then(|(_, t, _)| t),
        screenshot_base64,
        last_error,
    })
}

pub fn add_url(raw_url: &str, space_id: i64) -> Result<String> {
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
            space_id: Some(space_id),
        },
    )?;
    Ok(format!("[{}] {}", category, raw_url))
}

pub fn search_pages(query: &str, space_id: i64, limit: usize) -> Result<Vec<PageRow>> {
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
        "SELECT wp.id, wp.url, wp.title, wp.domain
         FROM web_page_fts fts
         JOIN web_page_snapshot wps ON wps.id = fts.rowid
         JOIN web_page wp ON wp.id = wps.web_page_id
         WHERE web_page_fts MATCH ?1 AND wp.trashed_at IS NULL AND wp.space_id = ?2
         ORDER BY rank
         LIMIT ?3",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![query, space_id, limit as i64], |row| {
            Ok(PageRow {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                domain: row.get(3)?,
                status: String::new(),
                created_at: String::new(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn search_notes(query: &str, space_id: i64, limit: usize) -> Result<Vec<lore_core::db::NoteRow>> {
    let conn = open_db()?;
    let query = if query.contains('*') || query.contains('"') {
        query.to_string()
    } else {
        query
            .split_whitespace()
            .map(|w| format!("{}*", w))
            .collect::<Vec<_>>()
            .join(" ")
    };

    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, SUBSTR(n.body, 1, 100), n.folder_id, n.updated_at
         FROM note_fts fts
         JOIN note n ON n.id = fts.rowid
         WHERE note_fts MATCH ?1 AND n.deleted_at IS NULL AND n.space_id = ?2
         ORDER BY rank
         LIMIT ?3",
    )?;

    let rows = stmt
        .query_map(rusqlite::params![query, space_id, limit as i64], |row| {
            Ok(lore_core::db::NoteRow {
                id: row.get(0)?,
                title: row.get(1)?,
                body_preview: row.get(2)?,
                folder_id: row.get(3)?,
                updated_at: row.get::<_, String>(4)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// ---- Trash queries ----

#[derive(Clone, Debug)]
pub struct TrashItem {
    pub id: i64,
    pub title: String,
    pub kind: TrashKind,
    pub trashed_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TrashKind {
    Page,
    Note,
    File,
}

pub fn list_trash(space_id: i64) -> Result<Vec<TrashItem>> {
    let conn = open_db()?;
    let mut items = Vec::new();

    // Trashed pages
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(title, url), trashed_at FROM web_page WHERE trashed_at IS NOT NULL AND space_id = ?1 ORDER BY trashed_at DESC",
    )?;
    let pages = stmt
        .query_map([space_id], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                title: row.get(1)?,
                kind: TrashKind::Page,
                trashed_at: row.get(2)?,
            })
        })?
        .filter_map(|r| r.ok());
    items.extend(pages);

    // Trashed notes
    let mut stmt = conn.prepare(
        "SELECT id, title, deleted_at FROM note WHERE deleted_at IS NOT NULL AND space_id = ?1 ORDER BY deleted_at DESC",
    )?;
    let notes = stmt
        .query_map([space_id], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                title: row.get(1)?,
                kind: TrashKind::Note,
                trashed_at: row.get(2)?,
            })
        })?
        .filter_map(|r| r.ok());
    items.extend(notes);

    // Trashed files
    let mut stmt = conn.prepare(
        "SELECT id, name, deleted_at FROM file WHERE deleted_at IS NOT NULL AND space_id = ?1 ORDER BY deleted_at DESC",
    )?;
    let files = stmt
        .query_map([space_id], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                title: row.get(1)?,
                kind: TrashKind::File,
                trashed_at: row.get(2)?,
            })
        })?
        .filter_map(|r| r.ok());
    items.extend(files);

    // Sort by trashed_at desc
    items.sort_by(|a, b| b.trashed_at.cmp(&a.trashed_at));
    Ok(items)
}

// ---- File helpers ----

pub fn format_file_size(bytes: i64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Extract the uppercase file extension, e.g. "PDF", "PNG". Returns "FILE" if none.
pub fn file_extension(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "FILE".into())
        .to_string()
}

/// Best-effort MIME type from file extension.
pub fn mime_from_extension(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf"  => "application/pdf",
        "png"  => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif"  => "image/gif",
        "webp" => "image/webp",
        "svg"  => "image/svg+xml",
        "avif" => "image/avif",
        "txt"  => "text/plain",
        "md"   => "text/markdown",
        "html" | "htm" => "text/html",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc"  => "application/msword",
        "xls"  => "application/vnd.ms-excel",
        "zip"  => "application/zip",
        "gz"   => "application/gzip",
        "json" => "application/json",
        "xml"  => "application/xml",
        "csv"  => "text/csv",
        "mp4"  => "video/mp4",
        "mp3"  => "audio/mpeg",
        _      => "application/octet-stream",
    }
    .to_string()
}

/// Save file bytes to ~/Downloads/<name>. Returns the destination path.

// ---- Rules ----

#[derive(Clone, Debug)]
pub struct RuleRow {
    pub pattern: String,
    pub match_type: String,
    pub category: String,
    pub note: String,
}

pub fn load_rules() -> Result<Vec<RuleRow>> {
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

pub fn get_revision() -> i64 {
    open_db()
        .ok()
        .and_then(|conn| lore_core::db::get_revision(&conn).ok())
        .unwrap_or(0)
}

/// Read PRAGMA user_version directly (raw connection, no migrations, no
/// refuse-on-newer). Used by the polling loop to detect a schema upgrade
/// happening underneath us — going through `open_db()` would fail outright
/// if another process has bumped the DB past `EXPECTED_VERSION`.
pub fn db_schema_version() -> u32 {
    rusqlite::Connection::open(db_path())
        .ok()
        .and_then(|c| c.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).ok())
        .unwrap_or(0)
}

/// Extract URLs from text and auto-archive them
pub fn auto_archive_urls(text: &str, space_id: i64) {
    let conn = match open_db() {
        Ok(c) => c,
        Err(_) => return,
    };
    let rules = lore_core::db::load_rules(&conn).unwrap_or_default();

    let urls = extract_urls(text);
    for url in &urls {
        if lore_core::db::find_page_by_url(&conn, url).ok().flatten().is_none()
            && let Ok(parsed) = url::Url::parse(url) {
                let normalized = lore_core::rules::normalize_url(&parsed);
                let domain = parsed.host_str().unwrap_or("unknown").to_string();
                let category = lore_core::rules::classify(&parsed, &rules);
                let status = if category == "archive" { "queued" } else { "skipped" };
                lore_core::db::insert_web_page(&conn, &lore_core::db::NewWebPage {
                    url,
                    url_normalized: &normalized,
                    title: None,
                    domain: &domain,
                    category: &category,
                    status,
                    source: Some("note"),
                    space_id: Some(space_id),
                }).ok();
            }
    }
}

/// Extract http/https URLs from markdown text
pub fn extract_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();

    // Pattern 1: [text](url)
    let mut rest = text;
    while let Some(pos) = rest.find("](") {
        let start = pos + 2;
        if let Some(end) = rest[start..].find(')') {
            let url = rest[start..start + end].trim();
            if (url.starts_with("http://") || url.starts_with("https://"))
                && !urls.contains(&url.to_string()) {
                    urls.push(url.to_string());
                }
            rest = &rest[start + end..];
        } else {
            break;
        }
    }

    // Pattern 2: bare URLs
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| c == '(' || c == ')' || c == '<' || c == '>' || c == '"' || c == '\'' || c == ',' || c == ';' || c == '.');
        if (word.starts_with("http://") || word.starts_with("https://")) && !urls.contains(&word.to_string()) {
            urls.push(word.to_string());
        }
    }

    urls
}

pub fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
