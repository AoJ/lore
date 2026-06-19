//! Markdown-folder import: pour a directory tree of `.md` files into a space as
//! notes, **idempotently**. Identity = `<root_folder>/<path relative to the
//! import root>` (stored on `note.import_source`), so re-importing the same
//! folder updates changed files and skips unchanged ones instead of duplicating
//! — and refuses to clobber a note you edited inside lore. Including the root
//! folder in the identity namespaces separate imports and scopes `--prune`.
//!
//! Two hashes drive a robust three-way decision because the stored body differs
//! from the raw file once local links become attachments:
//!
//! - `import_hash` = hash of the RAW source file → "did the file change?"
//! - `import_rendered_hash` = hash of the stored (attachment-rewritten) body →
//!   "did I edit it in lore?"; falls back to `import_hash` for notes imported
//!   before attachment support (their stored body == the raw file).
//!
//! | condition                          | action   |
//! |------------------------------------|----------|
//! | no note for this path              | insert   |
//! | edited in lore                     | conflict |
//! | not edited, file changed           | update   |
//! | not edited, file unchanged         | skip     |
//!
//! Any conflict aborts the whole import (transaction rolled back, nothing
//! written). Local links/images (`![](rel)`, `[](rel)`) pointing at existing
//! files become note attachments, the link rewritten to the lore attachment URL.
//! With `--prune`, imported notes under this root whose source file vanished are
//! trashed.
//!
//! Out of scope: parsing per-file created/updated timestamps; %-encoded or
//! parenthesised link targets.

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::db::{
    find_imported_note, get_or_create_folder, insert_attachment, insert_imported_note, trash_note,
    update_imported_note,
};

/// Summary of an import run. On a real (non-dry-run) import the counts reflect
/// what was committed; on a dry run they reflect what *would* happen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub attachments: usize,
    pub pruned: usize,
    /// Source paths where the note was edited in lore and the file also differs.
    /// Left untouched; aborts a real import.
    pub conflicts: Vec<String>,
}

/// Per-file decision. Pure, so it's unit-testable without a DB.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Insert,
    Update,
    Skip,
    Conflict,
}

fn decide(existing: bool, file_changed: bool, lore_edited: bool) -> Action {
    if !existing {
        Action::Insert
    } else if lore_edited {
        Action::Conflict
    } else if file_changed {
        Action::Update
    } else {
        Action::Skip
    }
}

/// Normalise line endings so CRLF/LF differences don't read as content changes.
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn sha256(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

/// Title = the first non-blank line if it's an ATX heading (`# ...`), else
/// `None` (caller falls back to the file name).
fn extract_title(body: &str) -> Option<String> {
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let rest = t.strip_prefix('#')?; // first non-blank line must be a heading
        let title = rest.trim_start_matches('#').trim();
        return if title.is_empty() {
            None
        } else {
            Some(title.to_string())
        };
    }
    None
}

fn file_stem_title(rel: &Path) -> String {
    rel.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "untitled".to_string())
}

/// Posix-style relative path string — stable, cross-platform.
fn rel_to_str(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading dir {}", dir.display()))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue; // skip dotfiles/dirs
        }
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk(root, &path, out)?;
        } else if ty.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_path_buf());
        }
    }
    Ok(())
}

fn ensure_folder_path(
    conn: &Connection,
    space_id: i64,
    root_folder: &str,
    rel: &Path,
    cache: &mut HashMap<PathBuf, i64>,
) -> Result<i64> {
    let root_key = PathBuf::new();
    let mut current = if let Some(&id) = cache.get(&root_key) {
        id
    } else {
        let id = get_or_create_folder(conn, root_folder, None, space_id)?;
        cache.insert(root_key, id);
        id
    };
    if let Some(parent) = rel.parent() {
        let mut key = PathBuf::new();
        for comp in parent.components() {
            let name = comp.as_os_str().to_string_lossy().into_owned();
            key.push(&name);
            if let Some(&id) = cache.get(&key) {
                current = id;
                continue;
            }
            current = get_or_create_folder(conn, &name, Some(current), space_id)?;
            cache.insert(key.clone(), current);
        }
    }
    Ok(current)
}

// Capture group 1 = the link/image target inside `](...)`. Handles the common
// `](path)` and `](path "title")`; not angle-bracket or parenthesised targets.
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]\(\s*([^)\s]+)").unwrap());

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("txt" | "log") => "text/plain",
        Some("md") => "text/markdown",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("html" | "htm") => "text/html",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

/// If `target` is a relative path to an existing file under `base_dir`, return
/// it; otherwise `None` (URLs, anchors, absolute paths, missing files).
fn resolve_local(target: &str, base_dir: &Path) -> Option<PathBuf> {
    let t = target.trim();
    if t.is_empty()
        || t.starts_with('#')
        || t.starts_with('<')
        || t.starts_with('/')
        || t.starts_with("//")
        || t.contains("://")
        || t.starts_with("mailto:")
        || t.starts_with("tel:")
    {
        return None;
    }
    let candidate = base_dir.join(t);
    candidate.is_file().then_some(candidate)
}

/// Ingest local files referenced by `body` as attachments of `note_id`, rewriting
/// each link to the lore attachment URL. Returns the new body + how many were
/// converted. `insert_attachment` dedupes, so this is safe to re-run.
fn render_attachments(
    conn: &Connection,
    note_id: i64,
    body: &str,
    base_dir: &Path,
) -> Result<(String, usize)> {
    let mut out = String::with_capacity(body.len());
    let mut last = 0usize;
    let mut count = 0usize;
    for cap in LINK_RE.captures_iter(body) {
        let target = cap.get(1).unwrap();
        let Some(local) = resolve_local(target.as_str(), base_dir) else {
            continue; // not a local file — leave the link as-is
        };
        let bytes =
            std::fs::read(&local).with_context(|| format!("reading {}", local.display()))?;
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string());
        let (att_id, _) = insert_attachment(conn, note_id, &name, mime_for(&local), &bytes)?;
        out.push_str(&body[last..target.start()]);
        out.push_str(&format!("https://attachment.lore.invalid/{att_id}"));
        last = target.end();
        count += 1;
    }
    out.push_str(&body[last..]);
    Ok((out, count))
}

/// Import every `.md` under `root` into `space_id`, mirroring the directory tree
/// under a top-level `root_folder`. See the module docs for the conflict model.
/// `dry_run` rolls back at the end (and reports conflicts instead of erroring);
/// `prune` trashes imported notes under this root whose source file vanished.
pub fn import_markdown_dir(
    conn: &mut Connection,
    root: &Path,
    space_id: i64,
    root_folder: &str,
    dry_run: bool,
    prune: bool,
) -> Result<ImportReport> {
    let files = collect_markdown_files(root)?;
    let tx = conn.transaction()?;
    let mut report = ImportReport::default();
    let mut folder_cache: HashMap<PathBuf, i64> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

    for rel in &files {
        let abs = root.join(rel);
        let base_dir = abs.parent().unwrap_or(root).to_path_buf();
        let raw =
            std::fs::read_to_string(&abs).with_context(|| format!("reading {}", abs.display()))?;
        let body = normalize(&raw);
        let file_hash = sha256(&body);
        let title = extract_title(&body).unwrap_or_else(|| file_stem_title(rel));
        let import_source = format!("{}/{}", root_folder, rel_to_str(rel));
        seen.insert(import_source.clone());

        let existing = find_imported_note(&tx, space_id, &import_source)?;
        let action = match &existing {
            None => Action::Insert,
            Some(n) => {
                let lore_hash = sha256(&normalize(&n.body));
                let base_rendered = n
                    .import_rendered_hash
                    .as_deref()
                    .or(n.import_hash.as_deref());
                let lore_edited = Some(lore_hash.as_str()) != base_rendered;
                let file_changed = n.import_hash.as_deref() != Some(file_hash.as_str());
                decide(true, file_changed, lore_edited)
            }
        };

        match action {
            Action::Insert => {
                let folder_id =
                    ensure_folder_path(&tx, space_id, root_folder, rel, &mut folder_cache)?;
                let id = insert_imported_note(
                    &tx,
                    &title,
                    &body,
                    Some(folder_id),
                    space_id,
                    &import_source,
                    &file_hash,
                    &sha256(&body),
                )?;
                let (rendered, n_att) = render_attachments(&tx, id, &body, &base_dir)?;
                if rendered != body {
                    update_imported_note(
                        &tx,
                        id,
                        &title,
                        &rendered,
                        &file_hash,
                        &sha256(&normalize(&rendered)),
                    )?;
                }
                report.attachments += n_att;
                report.inserted += 1;
            }
            Action::Update => {
                let id = existing.as_ref().unwrap().id;
                let (rendered, n_att) = render_attachments(&tx, id, &body, &base_dir)?;
                update_imported_note(
                    &tx,
                    id,
                    &title,
                    &rendered,
                    &file_hash,
                    &sha256(&normalize(&rendered)),
                )?;
                report.attachments += n_att;
                report.updated += 1;
            }
            Action::Skip => report.skipped += 1,
            Action::Conflict => report.conflicts.push(import_source),
        }
    }

    if prune {
        let prefix = format!("{}/", root_folder);
        let mut stmt = tx.prepare(
            "SELECT id, import_source FROM note \
             WHERE space_id = ?1 AND import_source IS NOT NULL AND deleted_at IS NULL",
        )?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([space_id], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        for (id, src) in rows {
            if src.starts_with(&prefix) && !seen.contains(&src) {
                trash_note(&tx, id)?;
                report.pruned += 1;
            }
        }
    }

    if dry_run {
        drop(tx); // roll back — preview only
        return Ok(report);
    }
    if !report.conflicts.is_empty() {
        drop(tx); // atomic abort — write nothing
        return Err(anyhow!(
            "import aborted: {} note(s) were edited in lore and differ from the source \
             (resolve in lore or re-export, then re-run):\n  {}",
            report.conflicts.len(),
            report.conflicts.join("\n  ")
        ));
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn test_db() -> (Connection, i64) {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::migrations::apply(&mut conn).unwrap();
        let space_id = crate::db::insert_space(&conn, "Test").unwrap();
        (conn, space_id)
    }

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TmpDir(PathBuf);
    impl TmpDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let p = std::env::temp_dir().join(format!("lore-import-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&p).unwrap();
            TmpDir(p)
        }
        fn write(&self, rel: &str, contents: &[u8]) {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        fn remove(&self, rel: &str) {
            std::fs::remove_file(self.0.join(rel)).unwrap();
        }
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn decide_covers_all_cases() {
        assert_eq!(decide(false, false, false), Action::Insert);
        assert_eq!(decide(true, false, false), Action::Skip);
        assert_eq!(decide(true, true, false), Action::Update);
        assert_eq!(decide(true, false, true), Action::Conflict);
        assert_eq!(decide(true, true, true), Action::Conflict); // edited wins over changed
    }

    #[test]
    fn extract_title_prefers_heading_then_filename() {
        assert_eq!(extract_title("# Hello\n\nbody"), Some("Hello".into()));
        assert_eq!(extract_title("\n\n## Deep\n"), Some("Deep".into()));
        assert_eq!(extract_title("just prose\n# later"), None);
        assert_eq!(extract_title("#\nno text"), None);
    }

    #[test]
    fn insert_then_reimport_is_idempotent() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", b"# Alpha\n\nbody a");
        dir.write("sub/b.md", b"# Beta\n\nbody b");

        let r1 = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();
        assert_eq!((r1.inserted, r1.updated, r1.skipped), (2, 0, 0));

        let r2 = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();
        assert_eq!((r2.inserted, r2.updated, r2.skipped), (0, 0, 2));

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note WHERE space_id = ?1",
                [space],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        // identity is namespaced with the root folder
        let title: String = conn
            .query_row(
                "SELECT title FROM note WHERE import_source = 'docs/sub/b.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "Beta");
    }

    #[test]
    fn changed_file_updates_when_lore_untouched() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", b"# A\n\nv1");
        import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();
        dir.write("a.md", b"# A\n\nv2");
        let r = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();
        assert_eq!((r.inserted, r.updated, r.skipped), (0, 1, 0));
        let body: String = conn
            .query_row(
                "SELECT body FROM note WHERE import_source = 'docs/a.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(body, "# A\n\nv2");
    }

    #[test]
    fn edited_in_lore_then_reimport_conflicts_and_writes_nothing() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", b"# A\n\norig");
        dir.write("b.md", b"# B\n\nb");
        import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();

        conn.execute(
            "UPDATE note SET body = 'edited' WHERE import_source = 'docs/a.md'",
            [],
        )
        .unwrap();
        dir.write("a.md", b"# A\n\nnew");
        dir.write("b.md", b"# B\n\nb changed");

        let err = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap_err();
        assert!(err.to_string().contains("docs/a.md"));

        let a: String = conn
            .query_row(
                "SELECT body FROM note WHERE import_source = 'docs/a.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(a, "edited");
        let b: String = conn
            .query_row(
                "SELECT body FROM note WHERE import_source = 'docs/b.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b, "# B\n\nb"); // atomic abort: b untouched
    }

    #[test]
    fn local_link_becomes_attachment_and_reimport_skips() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("img.png", b"\x89PNG\r\n\x1a\nfake png bytes");
        dir.write(
            "note.md",
            b"# Pic\n\nhere: ![alt](img.png) and [doc](http://x.com)\n",
        );

        let r = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();
        assert_eq!(r.inserted, 1);
        assert_eq!(r.attachments, 1);

        let (body, nid): (String, i64) = conn
            .query_row(
                "SELECT body, id FROM note WHERE import_source = 'docs/note.md'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(body.contains("https://attachment.lore.invalid/"));
        assert!(body.contains("http://x.com")); // external link left alone
        let att: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_attachment WHERE note_id = ?1",
                [nid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(att, 1);

        // re-import: unchanged → skip, NO duplicate attachment
        let r2 = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();
        assert_eq!((r2.skipped, r2.attachments), (1, 0));
        let att2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM note_attachment WHERE note_id = ?1",
                [nid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(att2, 1);
    }

    #[test]
    fn prune_trashes_vanished_sources() {
        let (mut conn, space) = test_db();
        let dir = TmpDir::new();
        dir.write("a.md", b"# A\n\na");
        dir.write("b.md", b"# B\n\nb");
        import_markdown_dir(&mut conn, &dir.0, space, "docs", false, false).unwrap();

        dir.remove("b.md");
        let r = import_markdown_dir(&mut conn, &dir.0, space, "docs", false, true).unwrap();
        assert_eq!(r.pruned, 1);
        let deleted: Option<String> = conn
            .query_row(
                "SELECT deleted_at FROM note WHERE import_source = 'docs/b.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(deleted.is_some()); // b trashed
        let a_live: Option<String> = conn
            .query_row(
                "SELECT deleted_at FROM note WHERE import_source = 'docs/a.md'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(a_live.is_none()); // a kept
    }
}
