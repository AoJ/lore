use anyhow::Result;
use rusqlite::Connection;

use super::file::delete_file_permanent;
use super::note::delete_note_permanent;
use super::web_page::delete_page;

/// Permanently delete a space and ALL its content. Lives here (not in space.rs)
/// because it orchestrates deletes across web_page / note / file / folder —
/// keeping it among cross-entity cleanup keeps entity submodules independent.
pub fn delete_space_permanent(conn: &Connection, space_id: i64) -> Result<()> {
    let page_ids: Vec<i64> = conn
        .prepare("SELECT id FROM web_page WHERE space_id = ?1")?
        .query_map([space_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for pid in page_ids {
        delete_page(conn, pid)?;
    }
    let note_ids: Vec<i64> = conn
        .prepare("SELECT id FROM note WHERE space_id = ?1")?
        .query_map([space_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for nid in note_ids {
        delete_note_permanent(conn, nid)?;
    }
    conn.execute("DELETE FROM file WHERE space_id = ?1", [space_id])?;
    conn.execute("DELETE FROM note_folder WHERE space_id = ?1", [space_id])?;
    conn.execute("DELETE FROM space WHERE id = ?1", [space_id])?;
    Ok(())
}

/// Trash count across all entities (pages, notes, files) for a space.
pub fn trash_count(conn: &Connection, space_id: i64) -> Result<i64> {
    conn.query_row(
        "SELECT \
            (SELECT COUNT(*) FROM web_page WHERE trashed_at IS NOT NULL AND space_id = ?1) + \
            (SELECT COUNT(*) FROM note      WHERE deleted_at IS NOT NULL AND space_id = ?1) + \
            (SELECT COUNT(*) FROM file      WHERE deleted_at IS NOT NULL AND space_id = ?1)",
        [space_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

/// Hard-delete trashed items older than `days`. Touches every entity that has
/// a soft-delete column plus note_attachment.
pub fn cleanup_old_trash(conn: &Connection, days: i64) -> Result<usize> {
    let cutoff = format!("-{} days", days);
    let mut cleaned = 0usize;

    // Clean old trashed pages
    let page_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM web_page WHERE trashed_at IS NOT NULL AND trashed_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        )?
        .query_map([&cutoff], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in &page_ids {
        delete_page(conn, *id)?;
        cleaned += 1;
    }

    // Clean old trashed notes
    let note_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM note WHERE deleted_at IS NOT NULL AND deleted_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        )?
        .query_map([&cutoff], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in &note_ids {
        delete_note_permanent(conn, *id)?;
        cleaned += 1;
    }

    // Clean old trashed files
    let file_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM file WHERE deleted_at IS NOT NULL AND deleted_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        )?
        .query_map([&cutoff], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in &file_ids {
        delete_file_permanent(conn, *id)?;
        cleaned += 1;
    }

    // Clean old trashed spaces (permanently delete with all content)
    let space_ids: Vec<i64> = conn
        .prepare(
            "SELECT id FROM space WHERE deleted_at IS NOT NULL AND deleted_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        )?
        .query_map([&cutoff], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    for id in &space_ids {
        delete_space_permanent(conn, *id)?;
        cleaned += 1;
    }

    // Clean old soft-deleted note attachments (BLOB hard-delete after retention)
    let att_cleaned = conn.execute(
        "DELETE FROM note_attachment WHERE deleted_at IS NOT NULL AND deleted_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        [&cutoff],
    )?;
    cleaned += att_cleaned;

    Ok(cleaned)
}
