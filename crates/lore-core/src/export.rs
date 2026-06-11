//! Snapshot export — produce a portable file (HTML / Markdown / JSON) from
//! a single archived snapshot. Used by the UI's Export menu on page detail.
//!
//! Three formats:
//!   - HTML: self-contained, opens in any browser offline. Inlines title,
//!     meta, readability article (or plain_text fallback), embedded
//!     thumbnail, and the raw HTML inside a collapsed `<details>`.
//!   - Markdown: YAML frontmatter + readability article converted via
//!     `htmd` (or plain_text as fallback). Drop-in for Obsidian / Logseq.
//!   - JSON: every snapshot field. Suitable for downstream pipelines —
//!     binary blobs (screenshot, thumb) are base64-encoded inline.
//!
//! Each format returns an [`Exported`] containing both the suggested
//! filename and the byte payload, so callers don't have to know the
//! per-format extension or naming convention.

#[cfg(feature = "sqlite")]
use anyhow::{Context, Result};
#[cfg(feature = "sqlite")]
use rusqlite::Connection;
#[cfg(feature = "sqlite")]
use serde::Serialize;

/// Output format selector. Matches the Export menu in `content_page.rs`.
///
/// Always available (not behind `sqlite`) so the WASM client can pass it
/// to the backend without pulling in the SQL-touching machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Html,
    Markdown,
    Json,
}

impl Format {
    /// File extension (no dot) for the chosen format.
    #[cfg(feature = "sqlite")]
    fn ext(&self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Markdown => "md",
            Format::Json => "json",
        }
    }

    /// MIME type for HTTP `Content-Type` headers when the export is
    /// streamed over the wire (raw download endpoint).
    pub fn mime(&self) -> &'static str {
        match self {
            Format::Html => "text/html; charset=utf-8",
            Format::Markdown => "text/markdown; charset=utf-8",
            Format::Json => "application/json; charset=utf-8",
        }
    }
}

/// Result of one export call: filename + bytes.
#[cfg(feature = "sqlite")]
#[derive(Debug, Clone)]
pub struct Exported {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// Export one snapshot to the requested format. Loads page + snapshot
/// from DB; bails with `not_found` semantics if the snapshot id is bogus.
#[cfg(feature = "sqlite")]
pub fn export_snapshot(conn: &Connection, snapshot_id: i64, format: Format) -> Result<Exported> {
    let snap = load_full_snapshot(conn, snapshot_id)?;
    let bytes = match format {
        Format::Html => render_html(&snap).into_bytes(),
        Format::Markdown => render_markdown(&snap).into_bytes(),
        Format::Json => render_json(&snap)?,
    };
    let filename = build_filename(&snap, format);
    Ok(Exported { filename, bytes })
}

// ---- Internal: load + format ----

/// Everything an export needs in one query. Pulled out of
/// `web_page_snapshot` joined with `web_page` so we don't issue 4
/// separate queries for one export.
#[cfg(feature = "sqlite")]
struct FullSnapshot {
    page_id: i64,
    snapshot_id: i64,
    version: i64,
    url: String,
    title: String,
    domain: String,
    fetched_at: String,
    content_hash: Option<String>,
    html_content: String,
    plain_text: String,
    readability_html: Option<String>,
    readability_text: Option<String>,
    screenshot_thumb: Option<Vec<u8>>,
    screenshot_full: Option<Vec<u8>>,
}

#[cfg(feature = "sqlite")]
fn load_full_snapshot(conn: &Connection, snapshot_id: i64) -> Result<FullSnapshot> {
    conn.query_row(
        "SELECT s.id, s.web_page_id, s.version, s.fetched_at, s.content_hash, \
                COALESCE(s.title, p.title, ''), p.url, p.domain, \
                COALESCE(s.html_content, ''), COALESCE(s.plain_text, ''), \
                s.readability_html, s.readability_text, \
                s.screenshot_thumb, s.screenshot \
         FROM web_page_snapshot s JOIN web_page p ON p.id = s.web_page_id \
         WHERE s.id = ?1",
        [snapshot_id],
        |row| {
            Ok(FullSnapshot {
                snapshot_id: row.get(0)?,
                page_id: row.get(1)?,
                version: row.get(2)?,
                fetched_at: row.get(3)?,
                content_hash: row.get(4)?,
                title: row.get(5)?,
                url: row.get(6)?,
                domain: row.get(7)?,
                html_content: row.get(8)?,
                plain_text: row.get(9)?,
                readability_html: row.get(10)?,
                readability_text: row.get(11)?,
                screenshot_thumb: row.get(12)?,
                screenshot_full: row.get(13)?,
            })
        },
    )
    .with_context(|| format!("snapshot {} not found", snapshot_id))
}

// ---- HTML ----

#[cfg(feature = "sqlite")]
fn render_html(s: &FullSnapshot) -> String {
    use base64::Engine as _;
    let html_title = html_escape(&s.title);
    let html_url = html_escape(&s.url);
    let html_domain = html_escape(&s.domain);
    let html_fetched = html_escape(&s.fetched_at);
    let screenshot_b64 = s
        .screenshot_thumb
        .as_ref()
        .or(s.screenshot_full.as_ref())
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b));

    let body = if let Some(ref article) = s.readability_html {
        // dom_smoothie HTML is already a fragment. We trust it here
        // because the file is opened by the user offline — same level of
        // exposure as opening the original page in a browser, minus the
        // network requests.
        article.clone()
    } else {
        format!("<pre>{}</pre>", html_escape(&s.plain_text))
    };

    let screenshot_block = match screenshot_b64 {
        Some(b64) => format!(
            "<details class=\"meta\"><summary>Screenshot</summary>\
             <img src=\"data:image/png;base64,{}\" alt=\"screenshot\"></details>",
            b64
        ),
        None => String::new(),
    };

    let raw_html_block = format!(
        "<details class=\"meta\"><summary>Raw HTML ({} bytes)</summary>\
         <pre><code>{}</code></pre></details>",
        s.html_content.len(),
        html_escape(&s.html_content),
    );

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\"><head>\
         <meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title>\
         <meta name=\"lore:url\" content=\"{url}\">\
         <meta name=\"lore:archived_at\" content=\"{fetched}\">\
         <meta name=\"lore:page_id\" content=\"{page_id}\">\
         <meta name=\"lore:snapshot_id\" content=\"{snapshot_id}\">\
         <meta name=\"lore:version\" content=\"{version}\">\
         <style>{css}</style>\
         </head><body>\
         <header class=\"lore-header\">\
         <h1>{title}</h1>\
         <p class=\"lore-source\">\u{1F310} <a href=\"{url}\">{url}</a></p>\
         <p class=\"lore-meta\">{domain} \u{00B7} archived {fetched} \u{00B7} v{version}</p>\
         </header>\
         <main class=\"lore-content\">{body}</main>\
         <footer class=\"lore-footer\">{screenshot}{raw_html}</footer>\
         </body></html>\n",
        title = html_title,
        url = html_url,
        fetched = html_fetched,
        page_id = s.page_id,
        snapshot_id = s.snapshot_id,
        version = s.version,
        css = HTML_EXPORT_CSS,
        domain = html_domain,
        body = body,
        screenshot = screenshot_block,
        raw_html = raw_html_block,
    )
}

/// Minimal serif-stack styling tuned for offline reading. No external
/// fonts; no JS. Width-capped so long lines stay readable.
#[cfg(feature = "sqlite")]
const HTML_EXPORT_CSS: &str = "\
body{font-family:Georgia,serif;line-height:1.6;max-width:42rem;\
margin:2rem auto;padding:0 1rem;color:#222}\
.lore-header{border-bottom:1px solid #ddd;margin-bottom:2rem;padding-bottom:1rem}\
.lore-header h1{margin:0 0 .5rem}\
.lore-source a{color:#0366d6;text-decoration:none}\
.lore-meta{color:#666;font-size:.9rem;margin:.25rem 0}\
.lore-content img{max-width:100%;height:auto}\
.lore-content pre{white-space:pre-wrap;word-break:break-word;\
background:#f6f8fa;padding:.75rem;border-radius:6px;font-size:.85rem}\
.lore-footer{margin-top:3rem;border-top:1px solid #ddd;padding-top:1rem}\
.lore-footer details{margin:.5rem 0}\
.lore-footer summary{cursor:pointer;color:#666}\
.lore-footer img{max-width:100%;height:auto;border:1px solid #ddd}\
";

/// HTML-escape for text nodes and attribute values. Belt-and-braces — five
/// characters cover both contexts, no per-position branching needed.
#[cfg(feature = "sqlite")]
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

// ---- Markdown ----

#[cfg(feature = "sqlite")]
fn render_markdown(s: &FullSnapshot) -> String {
    let body = match &s.readability_html {
        Some(html) => htmd::convert(html).unwrap_or_else(|_| s.plain_text.clone()),
        None => s.plain_text.clone(),
    };
    // YAML frontmatter — escape colons / quotes only where they'd break
    // the simple key:value layout. `serde_yaml` would be overkill for
    // five string fields.
    format!(
        "---\n\
         title: {title}\n\
         url: {url}\n\
         domain: {domain}\n\
         archived_at: {fetched}\n\
         version: {version}\n\
         page_id: {page_id}\n\
         snapshot_id: {snapshot_id}\n\
         ---\n\n\
         {body}\n",
        title = yaml_string(&s.title),
        url = yaml_string(&s.url),
        domain = yaml_string(&s.domain),
        fetched = yaml_string(&s.fetched_at),
        version = s.version,
        page_id = s.page_id,
        snapshot_id = s.snapshot_id,
        body = body,
    )
}

/// YAML string — wrap in double quotes and escape `\` + `"`. Keeps the
/// layout valid for values containing colons, special chars, or leading
/// whitespace, without pulling in a full YAML serializer.
#[cfg(feature = "sqlite")]
fn yaml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---- JSON ----

#[cfg(feature = "sqlite")]
#[derive(Debug, Serialize)]
struct JsonExport<'a> {
    page_id: i64,
    snapshot_id: i64,
    version: i64,
    url: &'a str,
    title: &'a str,
    domain: &'a str,
    archived_at: &'a str,
    content_hash: Option<&'a str>,
    plain_text: &'a str,
    readability_html: Option<&'a str>,
    readability_text: Option<&'a str>,
    html_content: &'a str,
    /// Thumbnail PNG bytes, base64-encoded. `None` when the snapshot has
    /// no thumbnail (legacy / HTTP-only).
    screenshot_thumb_b64: Option<String>,
    /// Full screenshot bytes, base64-encoded. Same nullability rules.
    screenshot_full_b64: Option<String>,
}

#[cfg(feature = "sqlite")]
fn render_json(s: &FullSnapshot) -> Result<Vec<u8>> {
    use base64::Engine as _;
    let dto = JsonExport {
        page_id: s.page_id,
        snapshot_id: s.snapshot_id,
        version: s.version,
        url: &s.url,
        title: &s.title,
        domain: &s.domain,
        archived_at: &s.fetched_at,
        content_hash: s.content_hash.as_deref(),
        plain_text: &s.plain_text,
        readability_html: s.readability_html.as_deref(),
        readability_text: s.readability_text.as_deref(),
        html_content: &s.html_content,
        screenshot_thumb_b64: s
            .screenshot_thumb
            .as_ref()
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b)),
        screenshot_full_b64: s
            .screenshot_full
            .as_ref()
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b)),
    };
    serde_json::to_vec_pretty(&dto).context("serialize export JSON")
}

// ---- Filename ----

/// `{domain}-{slug}-{YYYY-MM-DD}-{HHMMSS}.{ext}` — time is part of the
/// name because test workflows produce many versions per day and only-date
/// names would silently overwrite each other in Save dialogs.
#[cfg(feature = "sqlite")]
fn build_filename(s: &FullSnapshot, format: Format) -> String {
    let slug = slugify(&s.title);
    let stamp = compact_stamp(&s.fetched_at);
    let domain = slug_safe(&s.domain);
    if slug.is_empty() {
        format!(
            "{}-snapshot-{}-{}.{}",
            domain,
            s.snapshot_id,
            stamp,
            format.ext()
        )
    } else {
        format!("{}-{}-{}.{}", domain, slug, stamp, format.ext())
    }
}

/// `2026-05-21T14:30:52.123Z` → `2026-05-21-143052`. Best-effort: if the
/// stamp doesn't parse, fall back to the raw value with `:` stripped so
/// the filename is at least filesystem-legal.
#[cfg(feature = "sqlite")]
fn compact_stamp(iso: &str) -> String {
    let (date, time) = iso.split_once('T').unwrap_or((iso, ""));
    let time_clean: String = time
        .chars()
        .take_while(|&c| c != '.' && c != 'Z' && c != '+' && c != '-')
        .filter(|c| c.is_ascii_digit())
        .collect();
    if time_clean.is_empty() {
        date.replace(':', "")
    } else {
        format!("{}-{}", date, time_clean)
    }
}

/// Lowercase ASCII slug. Replaces runs of non-alphanumeric with `-`,
/// trims leading/trailing dashes, caps length so titanically-long titles
/// don't blow the filename limit on some filesystems.
#[cfg(feature = "sqlite")]
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_dash = true;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.chars().take(60).collect()
}

/// Domain `.` stays meaningful (`nytimes.com`) but other punctuation goes.
/// Slightly different rules from `slugify` because domains are short and
/// already URL-safe.
#[cfg(feature = "sqlite")]
fn slug_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewWebPage, ReadabilityBundle};

    #[test]
    fn html_escape_all_special_chars() {
        assert_eq!(html_escape("&"), "&amp;");
        assert_eq!(html_escape("<"), "&lt;");
        assert_eq!(html_escape(">"), "&gt;");
        assert_eq!(html_escape("\""), "&quot;");
        assert_eq!(html_escape("'"), "&#39;");
    }

    #[test]
    fn html_escape_mixed() {
        assert_eq!(
            html_escape("<script>alert('XSS')</script>"),
            "&lt;script&gt;alert(&#39;XSS&#39;)&lt;/script&gt;"
        );
    }

    #[test]
    fn html_escape_safe_chars_unchanged() {
        assert_eq!(html_escape("hello world 123"), "hello world 123");
        assert_eq!(html_escape("a-z A-Z 0-9"), "a-z A-Z 0-9");
    }

    #[test]
    fn yaml_string_escapes_backslash() {
        assert_eq!(yaml_string("path\\to\\file"), "\"path\\\\to\\\\file\"");
    }

    #[test]
    fn yaml_string_escapes_quote() {
        assert_eq!(yaml_string("say \"hello\""), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn yaml_string_escapes_newline() {
        assert_eq!(yaml_string("line1\nline2"), "\"line1\\nline2\"");
    }

    #[test]
    fn yaml_string_mixed() {
        assert_eq!(
            yaml_string("path\\\"line\nend"),
            "\"path\\\\\\\"line\\nend\""
        );
    }

    #[test]
    fn compute_change_summary_zero_prev_text() {
        // prev_text_size == 0, current == 0 → size_delta_pct = 0
        let summary = compute_change_summary("title", "title", 0, "body", 0, "hash1");
        assert!(summary.contains("\"size_delta_pct\":0"));

        // prev_text_size == 0, current > 0 → size_delta_pct = 100
        let summary = compute_change_summary("title", "title", 0, "body", 100, "hash2");
        assert!(summary.contains("\"size_delta_pct\":100"));
    }

    #[test]
    fn compute_change_summary_title_changed() {
        let summary = compute_change_summary("old", "new", 100, "text", 100, "hash");
        assert!(summary.contains("\"title_changed\":true"));

        let summary = compute_change_summary("same", "same", 100, "text", 100, "hash");
        assert!(summary.contains("\"title_changed\":false"));
    }

    #[test]
    fn compute_change_summary_content_same() {
        // prev_hash == current_hash
        let summary = compute_change_summary("title", "title", 100, "text", 100, "same_hash");
        assert!(summary.contains("\"content_same\":true"));

        // prev_hash != current_hash
        let summary = compute_change_summary("title", "title", 100, "text", 100, "different_hash");
        assert!(summary.contains("\"content_same\":false"));
    }

    fn seed(conn: &Connection, body: &str, readability: Option<&str>) -> i64 {
        let page_id = crate::db::insert_web_page(
            conn,
            &NewWebPage {
                url: "https://example.test/article",
                url_normalized: "https://example.test/article",
                title: Some("Hello World: A Story"),
                domain: "example.test",
                category: "archive",
                status: "archived",
                source: None,
                space_id: Some(1),
            },
        )
        .unwrap();
        crate::db::insert_snapshot(
            conn,
            page_id,
            "<html><body><h1>Hello</h1></body></html>",
            body,
            None,
            None,
            ReadabilityBundle {
                html: readability,
                text: readability.map(|_| body),
            },
        )
        .unwrap()
    }

    fn open_test_db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::TempDir::new().unwrap();
        let conn = crate::db::open(&dir.path().join("t.db")).unwrap();
        (dir, conn)
    }

    #[test]
    fn html_export_contains_title_and_body() {
        let (_dir, conn) = open_test_db();
        let sid = seed(
            &conn,
            "plain text body",
            Some("<article><p>article body</p></article>"),
        );
        let out = export_snapshot(&conn, sid, Format::Html).unwrap();
        assert!(out.filename.ends_with(".html"));
        let s = String::from_utf8(out.bytes).unwrap();
        assert!(s.contains("<title>Hello World: A Story</title>"));
        assert!(s.contains("article body"));
        // Meta tags carry IDs for downstream tooling
        assert!(s.contains("lore:snapshot_id"));
        assert!(s.contains("lore:url"));
    }

    #[test]
    fn html_export_uses_plain_text_when_no_readability() {
        let (_dir, conn) = open_test_db();
        let sid = seed(&conn, "fallback plain body", None);
        let out = export_snapshot(&conn, sid, Format::Html).unwrap();
        let s = String::from_utf8(out.bytes).unwrap();
        assert!(s.contains("fallback plain body"));
        // No readability → falls back to <pre>
        assert!(s.contains("<pre>fallback plain body</pre>"));
    }

    #[test]
    fn markdown_export_has_frontmatter_and_body() {
        let (_dir, conn) = open_test_db();
        let sid = seed(
            &conn,
            "plain text",
            Some("<article><h2>Header</h2><p>Para text</p></article>"),
        );
        let out = export_snapshot(&conn, sid, Format::Markdown).unwrap();
        assert!(out.filename.ends_with(".md"));
        let s = String::from_utf8(out.bytes).unwrap();
        assert!(s.starts_with("---\n"));
        assert!(s.contains("title: \"Hello World: A Story\""));
        assert!(s.contains("url: \"https://example.test/article\""));
        assert!(s.contains("## Header"));
        assert!(s.contains("Para text"));
    }

    #[test]
    fn json_export_round_trips_all_fields() {
        let (_dir, conn) = open_test_db();
        let sid = seed(&conn, "plain text body", Some("<article>x</article>"));
        let out = export_snapshot(&conn, sid, Format::Json).unwrap();
        assert!(out.filename.ends_with(".json"));
        let v: serde_json::Value = serde_json::from_slice(&out.bytes).unwrap();
        assert_eq!(v["url"], "https://example.test/article");
        assert_eq!(v["title"], "Hello World: A Story");
        assert_eq!(v["snapshot_id"], sid);
        assert_eq!(v["readability_html"], "<article>x</article>");
        assert!(v["content_hash"].is_string());
    }

    #[test]
    fn filename_includes_domain_slug_and_compact_timestamp() {
        let (_dir, conn) = open_test_db();
        let sid = seed(&conn, "x", None);
        let out = export_snapshot(&conn, sid, Format::Html).unwrap();
        assert!(
            out.filename
                .starts_with("example.test-hello-world-a-story-")
        );
        assert!(out.filename.ends_with(".html"));
        // Date + time joined by a dash (YYYY-MM-DD-HHMMSS)
        let middle = out
            .filename
            .strip_prefix("example.test-hello-world-a-story-")
            .unwrap()
            .strip_suffix(".html")
            .unwrap();
        // Format check: 2026-MM-DD-HHMMSS = 17 chars
        assert!(middle.len() >= 15 && middle.contains('-'));
    }

    #[test]
    fn export_returns_error_for_unknown_snapshot() {
        let (_dir, conn) = open_test_db();
        assert!(export_snapshot(&conn, 99999, Format::Html).is_err());
    }

    #[test]
    fn slugify_handles_unicode_and_specials() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(
            slugify("   leading and trailing   "),
            "leading-and-trailing"
        );
        assert_eq!(slugify("a/b\\c:d?e"), "a-b-c-d-e");
        // Non-ASCII drops out (no transliteration) but doesn't crash
        assert_eq!(slugify("Český titulek"), "esk-titulek");
    }
}
