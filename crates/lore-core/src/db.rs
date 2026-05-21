//! Database access layer. Public API is exposed flat (e.g. `db::insert_note`)
//! for backward compatibility with callers — submodules group queries by
//! domain to keep individual files focused.
//!
//! Types (`NoteRow`, `WebPageDetail`, `BackendError` etc.) compile in any
//! target, including browser WASM. The SQL-touching functions are gated
//! behind `feature = "sqlite"` so a WASM client can use `lore-core` for
//! its row/error shapes (deserialized from the HTTP API) without pulling
//! in `rusqlite` and a bundled C SQLite.

pub mod activity;
pub mod attachment;
pub mod file;
pub mod folder;
pub mod note;
pub mod space;
pub mod trash;
pub mod web_page;

// ---- Types — always available ----

pub use activity::PageRef;
pub use attachment::{AttachmentRow, InsertAttachmentOutcome};
pub use file::{FileRow, InsertFileOutcome};
pub use folder::FolderRow;
pub use note::{NoteData, NoteRow};
pub use space::{SpaceRow, SpaceStats};
pub use trash::{TrashItem, TrashKind};
pub use web_page::{
    ArchiveOutcome, ClassificationRule, SnapshotContent, SnapshotMeta, WebPageDetail, WebPageRow,
    WebPageSnapshot,
};

// `NewWebPage<'a>` is a thin SQL insert parameter — used only by callers
// going through the `sqlite` feature path. The WASM client wraps URL
// archiving through `archive_url` and never assembles one of these.
#[cfg(feature = "sqlite")]
pub use web_page::NewWebPage;

// ---- Functions — `sqlite` feature only ----

#[cfg(feature = "sqlite")]
pub use activity::{activity_by_day, activity_for_day};
#[cfg(feature = "sqlite")]
pub use attachment::{
    cleanup_orphaned_attachments, delete_attachments_for_note, get_attachment, get_attachment_data,
    insert_attachment, list_attachment_ids_for_note, list_attachments, list_removed_attachments,
    restore_attachment,
};
#[cfg(feature = "sqlite")]
pub use file::{
    delete_file_permanent, get_file, get_file_data, insert_file, list_files, list_trashed_files,
    restore_file, trash_file,
};
#[cfg(feature = "sqlite")]
pub use folder::{delete_folder, folder_note_counts, insert_folder, list_folders, rename_folder};
#[cfg(feature = "sqlite")]
pub use note::{
    delete_note_permanent, find_notes_referencing_url, get_note, insert_note,
    list_note_ids_ordered, list_notes, move_note_to_folder, restore_note, restore_note_safe,
    trash_note, update_note,
};
#[cfg(feature = "sqlite")]
pub use space::{
    get_active_space, insert_space, list_all_spaces, list_spaces, rename_space, restore_space,
    space_stats, touch_space, trash_space,
};
#[cfg(feature = "sqlite")]
pub use trash::{cleanup_old_trash, delete_space_permanent, list_trash, trash_count};
#[cfg(feature = "sqlite")]
pub use web_page::{
    archive_url, auto_archive_from_text, check_urls_status, delete_page, delete_page_version,
    ensure_page, find_page_by_url, get_page, get_page_version, get_snapshot_full_screenshot,
    insert_snapshot, insert_web_page, list_page_ids_ordered, list_page_versions, list_pages,
    load_rules, request_reachive, restore_page, trash_page, update_status,
    update_status_with_error,
};

#[cfg(feature = "sqlite")]
mod conn {
    use anyhow::{Context, Result};
    use rusqlite::Connection;
    use std::path::Path;

    use crate::migrations;

    const SEED: &str = include_str!("seed.sql");

    /// Open a connection without running migrations or seeding. Use this for
    /// short-lived ad-hoc opens after the DB has been bootstrapped (see `open`).
    /// Avoids running the migration check + seed counts on every poll tick.
    pub fn open_existing(path: &Path) -> Result<Connection> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        // foreign_keys is per-connection and must be set every time.
        // journal_mode is per-DB; applying it on each open is a no-op after bootstrap.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .context("setting connection pragmas")?;
        Ok(conn)
    }

    /// Bootstrap: open the DB, apply pending migrations, seed defaults if empty.
    /// Call once at app startup (or per CLI command). Subsequent runtime opens
    /// should go through `open_existing` to skip the migration runner overhead.
    pub fn open(path: &Path) -> Result<Connection> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory {}", parent.display()))?;
        }
        let mut conn = open_existing(path)?;

        // Apply schema migrations (or refuse if DB is from a newer build).
        migrations::apply(&mut conn).context("applying DB migrations")?;

        // Seed default space if none exists
        let space_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM space", [], |row| row.get(0))?;
        if space_count == 0 {
            conn.execute(
                "INSERT INTO space (name, last_used) VALUES ('Personal', strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                [],
            )?;
            // Assign all existing content to default space
            let default_id: i64 = conn.last_insert_rowid();
            conn.execute(
                "UPDATE web_page SET space_id = ?1 WHERE space_id IS NULL",
                [default_id],
            )?;
            conn.execute(
                "UPDATE note SET space_id = ?1 WHERE space_id IS NULL",
                [default_id],
            )?;
            conn.execute(
                "UPDATE note_folder SET space_id = ?1 WHERE space_id IS NULL",
                [default_id],
            )?;
        }

        // Seed classification rules if table is empty
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM classification_rule", [], |row| {
            row.get(0)
        })?;
        if count == 0 {
            conn.execute_batch(SEED)
                .context("seeding classification rules")?;
        }

        Ok(conn)
    }

    /// Get the current global revision number. Incremented by DB triggers on every change.
    pub fn get_revision(conn: &Connection) -> Result<i64> {
        conn.query_row("SELECT revision FROM db_revision WHERE id = 1", [], |r| {
            r.get(0)
        })
        .map_err(Into::into)
    }
}

#[cfg(feature = "sqlite")]
pub use conn::{get_revision, open, open_existing};
