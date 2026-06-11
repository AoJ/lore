use serde::{Deserialize, Serialize};

#[cfg(feature = "sqlite")]
use anyhow::Result;
#[cfg(feature = "sqlite")]
use rusqlite::Connection;
#[cfg(feature = "sqlite")]
use sha2::{Digest, Sha256};

/// Outcome of `insert_attachment` so the caller can show appropriate feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InsertAttachmentOutcome {
    Inserted,
    DedupedActive,
    RevivedFromRemoved,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachmentRow {
    pub id: i64,
    pub note_id: i64,
    pub name: String,
    pub mime_type: Option<String>,
    pub size: i64,
    pub hash: String,
    pub created_at: String,
    pub deleted_at: Option<String>,
}

/// Insert a note attachment with dedup against `(note_id, name, hash)`. Active
/// duplicate → return its ID. Removed-list duplicate → clear `deleted_at` and
/// return its ID. Same hash but different name = renamed version → new row.
#[cfg(feature = "sqlite")]
pub fn insert_attachment(
    conn: &Connection,
    note_id: i64,
    name: &str,
    mime_type: &str,
    data: &[u8],
) -> Result<(i64, InsertAttachmentOutcome)> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = format!("{:x}", hasher.finalize());
    let size = data.len() as i64;

    if let Ok(id) = conn.query_row(
        "SELECT id FROM note_attachment WHERE note_id = ?1 AND name = ?2 AND hash = ?3 AND deleted_at IS NULL",
        rusqlite::params![note_id, name, hash],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok((id, InsertAttachmentOutcome::DedupedActive));
    }

    if let Ok(id) = conn.query_row(
        "SELECT id FROM note_attachment WHERE note_id = ?1 AND name = ?2 AND hash = ?3 AND deleted_at IS NOT NULL",
        rusqlite::params![note_id, name, hash],
        |row| row.get::<_, i64>(0),
    ) {
        conn.execute("UPDATE note_attachment SET deleted_at = NULL WHERE id = ?1", [id])?;
        return Ok((id, InsertAttachmentOutcome::RevivedFromRemoved));
    }

    conn.execute(
        "INSERT INTO note_attachment (note_id, name, mime_type, size, hash, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![note_id, name, mime_type, size, hash, data],
    )?;
    Ok((conn.last_insert_rowid(), InsertAttachmentOutcome::Inserted))
}

#[cfg(feature = "sqlite")]
pub fn get_attachment_data(conn: &Connection, attachment_id: i64) -> Result<(String, Vec<u8>)> {
    conn.query_row(
        "SELECT mime_type, data FROM note_attachment WHERE id = ?1",
        [attachment_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
    )
    .map_err(Into::into)
}

#[cfg(feature = "sqlite")]
pub fn delete_attachments_for_note(conn: &Connection, note_id: i64) -> Result<()> {
    conn.execute("DELETE FROM note_attachment WHERE note_id = ?1", [note_id])?;
    Ok(())
}

#[cfg(feature = "sqlite")]
pub fn list_attachment_ids_for_note(conn: &Connection, note_id: i64) -> Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT id FROM note_attachment WHERE note_id = ?1 AND deleted_at IS NULL")?;
    let ids = stmt
        .query_map([note_id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}

/// Soft-delete attachments not referenced in current markdown body.
/// Sets `deleted_at = now`. Hard-delete is performed by `cleanup_old_trash`
/// after retention period expires.
#[cfg(feature = "sqlite")]
pub fn cleanup_orphaned_attachments(
    conn: &Connection,
    note_id: i64,
    used_ids: &[i64],
) -> Result<usize> {
    let all_ids = list_attachment_ids_for_note(conn, note_id)?;
    let mut deleted = 0;
    for id in &all_ids {
        if !used_ids.contains(id) {
            conn.execute(
                "UPDATE note_attachment SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
                [*id],
            )?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[cfg(feature = "sqlite")]
fn map_attachment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AttachmentRow> {
    Ok(AttachmentRow {
        id: row.get(0)?,
        note_id: row.get(1)?,
        name: row.get(2)?,
        mime_type: row.get(3)?,
        size: row.get(4)?,
        hash: row.get(5)?,
        created_at: row.get::<_, String>(6)?.chars().take(10).collect(),
        deleted_at: row.get(7)?,
    })
}

/// List active attachments for a note (deleted_at IS NULL).
#[cfg(feature = "sqlite")]
pub fn list_attachments(conn: &Connection, note_id: i64) -> Result<Vec<AttachmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, note_id, name, mime_type, size, hash, created_at, deleted_at \
         FROM note_attachment WHERE note_id = ?1 AND deleted_at IS NULL \
         ORDER BY created_at",
    )?;
    let rows = stmt
        .query_map([note_id], map_attachment_row)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

/// List soft-deleted attachments for a note (the "removed attachments" list).
#[cfg(feature = "sqlite")]
pub fn list_removed_attachments(conn: &Connection, note_id: i64) -> Result<Vec<AttachmentRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, note_id, name, mime_type, size, hash, created_at, deleted_at \
         FROM note_attachment WHERE note_id = ?1 AND deleted_at IS NOT NULL \
         ORDER BY deleted_at DESC",
    )?;
    let rows = stmt
        .query_map([note_id], map_attachment_row)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[cfg(feature = "sqlite")]
pub fn get_attachment(conn: &Connection, id: i64) -> Result<AttachmentRow> {
    conn.query_row(
        "SELECT id, note_id, name, mime_type, size, hash, created_at, deleted_at \
         FROM note_attachment WHERE id = ?1",
        [id],
        map_attachment_row,
    )
    .map_err(Into::into)
}

/// Clear `deleted_at` so the attachment is available again. Caller is
/// responsible for inserting the markdown reference back into the note body.
#[cfg(feature = "sqlite")]
pub fn restore_attachment(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE note_attachment SET deleted_at = NULL WHERE id = ?1",
        [id],
    )?;
    Ok(())
}
