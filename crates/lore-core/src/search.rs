//! FTS5 search. Pure query functions return data; the `cli` module formats
//! them for terminal output (used by `lore search` / `lore list`).

use anyhow::Result;
use rusqlite::Connection;

use crate::db::note::NoteRow;
use crate::db::web_page::WebPageRow;

/// FTS5 hit for a web page, including a highlighted snippet of the matching
/// plain-text region. The snippet wraps matches with `>>>` and `<<<`.
#[derive(Clone, Debug)]
pub struct WebPageHit {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub domain: String,
    pub category: String,
    pub snippet: Option<String>,
}

/// Full-text search across archived web pages in a single space.
pub fn search_web_pages(
    conn: &Connection,
    query: &str,
    space_id: i64,
    limit: usize,
) -> Result<Vec<WebPageHit>> {
    let prepared = prepare_query(query);
    let mut stmt = conn.prepare(
        "SELECT wp.id, wp.url, wp.title, wp.domain, wp.category, \
                highlight(web_page_fts, 1, '>>>', '<<<') as snip \
         FROM web_page_fts fts \
         JOIN web_page_snapshot wps ON wps.id = fts.rowid \
         JOIN web_page wp ON wp.id = wps.web_page_id \
         WHERE web_page_fts MATCH ?1 AND wp.trashed_at IS NULL AND wp.space_id = ?2 \
         ORDER BY rank \
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![prepared, space_id, limit as i64], |row| {
            Ok(WebPageHit {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                domain: row.get(3)?,
                category: row.get(4)?,
                snippet: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Light-weight web page row for callers that don't need a snippet.
/// Returns the same shape as `db::list_pages` so list views can show
/// search results in the same component.
pub fn search_web_pages_brief(
    conn: &Connection,
    query: &str,
    space_id: i64,
    limit: usize,
) -> Result<Vec<WebPageRow>> {
    let prepared = prepare_query(query);
    let mut stmt = conn.prepare(
        "SELECT wp.id, wp.title, wp.domain, wp.category, wp.status, wp.created_at \
         FROM web_page_fts fts \
         JOIN web_page_snapshot wps ON wps.id = fts.rowid \
         JOIN web_page wp ON wp.id = wps.web_page_id \
         WHERE web_page_fts MATCH ?1 AND wp.trashed_at IS NULL AND wp.space_id = ?2 \
         ORDER BY rank \
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![prepared, space_id, limit as i64], |row| {
            Ok(WebPageRow {
                id: row.get(0)?,
                title: row.get(1)?,
                domain: row.get(2)?,
                category: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Full-text search across notes in a single space.
pub fn search_notes(
    conn: &Connection,
    query: &str,
    space_id: i64,
    limit: usize,
) -> Result<Vec<NoteRow>> {
    let prepared = prepare_query(query);
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, SUBSTR(n.body, 1, 100), n.folder_id, n.updated_at \
         FROM note_fts fts \
         JOIN note n ON n.id = fts.rowid \
         WHERE note_fts MATCH ?1 AND n.deleted_at IS NULL AND n.space_id = ?2 \
         ORDER BY rank \
         LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![prepared, space_id, limit as i64], |row| {
            Ok(NoteRow {
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

/// Filtered, non-FTS list of web pages — used by `lore list` and
/// administrative views. `space_id = None` searches across all spaces.
pub fn list_pages_filtered(
    conn: &Connection,
    space_id: Option<i64>,
    category: Option<&str>,
    status: Option<&str>,
    domain: Option<&str>,
    limit: usize,
) -> Result<Vec<WebPageRow>> {
    let mut sql = String::from(
        "SELECT id, title, domain, category, status, created_at FROM web_page \
         WHERE trashed_at IS NULL",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(s) = space_id {
        sql.push_str(&format!(" AND space_id = ?{}", idx));
        params.push(Box::new(s));
        idx += 1;
    }
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
            Ok(WebPageRow {
                id: row.get(0)?,
                title: row.get(1)?,
                domain: row.get(2)?,
                category: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get::<_, String>(5)?.chars().take(10).collect(),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Auto-append `*` to simple FTS queries for prefix matching.
/// Leaves explicit FTS5 operators (`*`, `"`, AND/OR/NOT, column filters) untouched.
pub fn prepare_query(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.contains('*')
        || trimmed.contains('"')
        || trimmed.contains(" AND ")
        || trimmed.contains(" OR ")
        || trimmed.contains(" NOT ")
        || trimmed.contains(':')
    {
        return trimmed.to_string();
    }
    trimmed
        .split_whitespace()
        .map(|w| format!("{}*", w))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each bypass branch must be exercised independently so the OR chain at
    // lines 184..=189 can't survive being flipped to AND.

    #[test]
    fn prepare_simple_word_gets_auto_prefix() {
        assert_eq!(prepare_query("rust"), "rust*");
    }

    #[test]
    fn prepare_multi_word_each_gets_prefix() {
        assert_eq!(prepare_query("rust async"), "rust* async*");
    }

    #[test]
    fn prepare_trims_surrounding_whitespace() {
        assert_eq!(prepare_query("  rust  "), "rust*");
    }

    #[test]
    fn prepare_passthrough_with_explicit_wildcard() {
        // contains('*') branch
        assert_eq!(prepare_query("ru*"), "ru*");
    }

    #[test]
    fn prepare_passthrough_with_quoted_phrase() {
        // contains('"') branch
        assert_eq!(prepare_query("\"exact phrase\""), "\"exact phrase\"");
    }

    #[test]
    fn prepare_passthrough_with_boolean_and() {
        // contains(" AND ") branch
        assert_eq!(prepare_query("foo AND bar"), "foo AND bar");
    }

    #[test]
    fn prepare_passthrough_with_boolean_or() {
        // contains(" OR ") branch
        assert_eq!(prepare_query("foo OR bar"), "foo OR bar");
    }

    #[test]
    fn prepare_passthrough_with_boolean_not() {
        // contains(" NOT ") branch
        assert_eq!(prepare_query("foo NOT bar"), "foo NOT bar");
    }

    #[test]
    fn prepare_passthrough_with_column_filter() {
        // contains(':') branch
        assert_eq!(prepare_query("title:rust"), "title:rust");
    }

    #[test]
    fn prepare_empty_returns_empty() {
        assert_eq!(prepare_query(""), "");
        assert_eq!(prepare_query("   "), "");
    }
}
