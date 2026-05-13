use anyhow::Result;
use rusqlite::Connection;

use super::note::NoteRow;

/// (id, title) pair for a page summary used by timeline/activity views.
pub type PageRef = (i64, String);

/// Activity by day for heatmap (last N days)
pub fn activity_by_day(conn: &Connection, space_id: i64, days: i64) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT day, SUM(cnt) FROM (
            SELECT date(updated_at) as day, COUNT(*) as cnt
            FROM note WHERE space_id = ?1 AND deleted_at IS NULL
              AND updated_at > date('now', ?2)
            GROUP BY date(updated_at)
            UNION ALL
            SELECT date(created_at) as day, COUNT(*) as cnt
            FROM web_page WHERE space_id = ?1 AND trashed_at IS NULL
              AND created_at > date('now', ?2)
            GROUP BY date(created_at)
        ) GROUP BY day ORDER BY day",
    )?;
    let cutoff = format!("-{} days", days);
    let rows = stmt
        .query_map(rusqlite::params![space_id, cutoff], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// Get notes and pages active on a specific day
pub fn activity_for_day(conn: &Connection, space_id: i64, day: &str) -> Result<(Vec<NoteRow>, Vec<PageRef>)> {
    // Notes updated on this day
    let mut stmt = conn.prepare(
        "SELECT id, title, SUBSTR(body, 1, 100), folder_id, updated_at FROM note
         WHERE space_id = ?1 AND deleted_at IS NULL AND date(updated_at) = ?2
         ORDER BY updated_at DESC",
    )?;
    let notes: Vec<NoteRow> = stmt
        .query_map(rusqlite::params![space_id, day], |row| {
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

    // Pages created on this day
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(title, url) FROM web_page
         WHERE space_id = ?1 AND trashed_at IS NULL AND date(created_at) = ?2
         ORDER BY created_at DESC",
    )?;
    let pages: Vec<PageRef> = stmt
        .query_map(rusqlite::params![space_id, day], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok((notes, pages))
}
