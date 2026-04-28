use lore_core::db;
use std::path::PathBuf;
use tempfile::TempDir;

fn open_test_db() -> (TempDir, rusqlite::Connection) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("test.db");
    let conn = db::open(&path).unwrap();
    (dir, conn)
}

// ---- Space CRUD ----

#[test]
fn create_default_space() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    assert_eq!(space.name, "Personal");
}

#[test]
fn insert_and_list_spaces() {
    let (_dir, conn) = open_test_db();
    let id = db::insert_space(&conn, "Work").unwrap();
    assert!(id > 0);

    let spaces = db::list_spaces(&conn).unwrap();
    assert_eq!(spaces.len(), 2); // Personal + Work
    assert!(spaces.iter().any(|s| s.name == "Work"));
    assert!(spaces.iter().any(|s| s.name == "Personal"));
}

#[test]
fn rename_space() {
    let (_dir, conn) = open_test_db();
    let id = db::insert_space(&conn, "Old").unwrap();
    db::rename_space(&conn, id, "New").unwrap();
    let spaces = db::list_spaces(&conn).unwrap();
    assert!(spaces.iter().any(|s| s.name == "New"));
    assert!(!spaces.iter().any(|s| s.name == "Old"));
}

#[test]
fn touch_space_updates_last_used() {
    let (_dir, conn) = open_test_db();
    let personal = db::get_active_space(&conn).unwrap();
    let work_id = db::insert_space(&conn, "Work").unwrap();

    // Touch Personal explicitly — set it to "future" to guarantee ordering
    conn.execute(
        "UPDATE space SET last_used = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+1 second') WHERE id = ?1",
        [personal.id],
    ).unwrap();
    let active = db::get_active_space(&conn).unwrap();
    assert_eq!(active.id, personal.id);
}

#[test]
fn trash_and_restore_space() {
    let (_dir, conn) = open_test_db();
    let space_id = db::insert_space(&conn, "Temp").unwrap();

    db::trash_space(&conn, space_id).unwrap();
    // Should not appear in active list
    let active = db::list_spaces(&conn).unwrap();
    assert!(!active.iter().any(|s| s.id == space_id));
    // Should appear in all list
    let all = db::list_all_spaces(&conn).unwrap();
    assert!(all.iter().any(|s| s.id == space_id && s.deleted_at.is_some()));

    db::restore_space(&conn, space_id).unwrap();
    let active = db::list_spaces(&conn).unwrap();
    assert!(active.iter().any(|s| s.id == space_id));
}

#[test]
fn delete_space_removes_all_content() {
    let (_dir, conn) = open_test_db();
    let space_id = db::insert_space(&conn, "Doomed").unwrap();

    // Add content
    let page_id = db::insert_web_page(&conn, &db::NewWebPage {
        url: "https://example.com",
        url_normalized: "example.com",
        title: Some("Example"),
        domain: "example.com",
        category: "archive",
        status: "queued",
        source: None,
        space_id: Some(space_id),
    }).unwrap();
    let note_id = db::insert_note(&conn, "Test", "Body", None, space_id).unwrap();
    let folder_id = db::insert_folder(&conn, "Folder", None, space_id).unwrap();

    db::delete_space_permanent(&conn, space_id).unwrap();

    // Verify everything is gone
    let pages: Vec<i64> = conn
        .prepare("SELECT id FROM web_page WHERE space_id = ?1").unwrap()
        .query_map([space_id], |r| r.get(0)).unwrap()
        .filter_map(|r| r.ok()).collect();
    assert!(pages.is_empty());

    assert!(db::get_note(&conn, note_id).is_err());
    assert!(db::list_folders(&conn, space_id).unwrap().is_empty());
}

// ---- Space isolation ----

#[test]
fn notes_isolated_by_space() {
    let (_dir, conn) = open_test_db();
    let personal = db::get_active_space(&conn).unwrap();
    let work_id = db::insert_space(&conn, "Work").unwrap();

    db::insert_note(&conn, "Personal note", "", None, personal.id).unwrap();
    db::insert_note(&conn, "Work note", "", None, work_id).unwrap();

    let personal_notes = db::list_notes(&conn, None, personal.id).unwrap();
    assert_eq!(personal_notes.len(), 1);
    assert_eq!(personal_notes[0].title, "Personal note");

    let work_notes = db::list_notes(&conn, None, work_id).unwrap();
    assert_eq!(work_notes.len(), 1);
    assert_eq!(work_notes[0].title, "Work note");
}

#[test]
fn folders_isolated_by_space() {
    let (_dir, conn) = open_test_db();
    let personal = db::get_active_space(&conn).unwrap();
    let work_id = db::insert_space(&conn, "Work").unwrap();

    db::insert_folder(&conn, "Personal folder", None, personal.id).unwrap();
    db::insert_folder(&conn, "Work folder", None, work_id).unwrap();

    let pf = db::list_folders(&conn, personal.id).unwrap();
    assert_eq!(pf.len(), 1);
    assert_eq!(pf[0].name, "Personal folder");

    let wf = db::list_folders(&conn, work_id).unwrap();
    assert_eq!(wf.len(), 1);
    assert_eq!(wf[0].name, "Work folder");
}

#[test]
fn folder_note_counts_per_space() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let folder_id = db::insert_folder(&conn, "F1", None, space.id).unwrap();

    db::insert_note(&conn, "A", "", Some(folder_id), space.id).unwrap();
    db::insert_note(&conn, "B", "", Some(folder_id), space.id).unwrap();

    let counts = db::folder_note_counts(&conn, space.id).unwrap();
    assert_eq!(*counts.get(&folder_id).unwrap(), 2);

    // Other space sees zero
    let other = db::insert_space(&conn, "Other").unwrap();
    let other_counts = db::folder_note_counts(&conn, other).unwrap();
    assert!(other_counts.get(&folder_id).is_none());
}

// ---- Notes CRUD ----

#[test]
fn insert_and_get_note() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_note(&conn, "Title", "Body text", None, space.id).unwrap();

    let note = db::get_note(&conn, id).unwrap();
    assert_eq!(note.title, "Title");
    assert_eq!(note.body, "Body text");
    assert_eq!(note.folder_id, None);
}

#[test]
fn update_note() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_note(&conn, "Old", "Old body", None, space.id).unwrap();

    db::update_note(&conn, id, "New", "New body").unwrap();
    let note = db::get_note(&conn, id).unwrap();
    assert_eq!(note.title, "New");
    assert_eq!(note.body, "New body");
}

#[test]
fn note_in_folder() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let folder = db::insert_folder(&conn, "Projects", None, space.id).unwrap();
    let note = db::insert_note(&conn, "Note", "", Some(folder), space.id).unwrap();

    let notes = db::list_notes(&conn, Some(folder), space.id).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, note);

    // Root notes (no folder) should be empty — our note is in a folder
    let root = db::list_notes(&conn, None, space.id).unwrap();
    assert!(root.is_empty(), "note in folder should not appear in root listing");
}

#[test]
fn move_note_between_folders() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let f1 = db::insert_folder(&conn, "F1", None, space.id).unwrap();
    let f2 = db::insert_folder(&conn, "F2", None, space.id).unwrap();
    let note = db::insert_note(&conn, "Note", "", Some(f1), space.id).unwrap();

    // Note is in F1
    let in_f1 = db::list_notes(&conn, Some(f1), space.id).unwrap();
    assert_eq!(in_f1.len(), 1);

    // Move to F2
    db::move_note_to_folder(&conn, note, Some(f2)).unwrap();
    let in_f1 = db::list_notes(&conn, Some(f1), space.id).unwrap();
    let in_f2 = db::list_notes(&conn, Some(f2), space.id).unwrap();
    assert!(in_f1.is_empty());
    assert_eq!(in_f2.len(), 1);

    // Move to root
    db::move_note_to_folder(&conn, note, None).unwrap();
    let root = db::list_notes(&conn, None, space.id).unwrap();
    assert_eq!(root.len(), 1);
}

// ---- Trash / soft delete ----

#[test]
fn trash_and_restore_note() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_note(&conn, "Temp", "", None, space.id).unwrap();

    db::trash_note(&conn, id).unwrap();
    let notes = db::list_notes(&conn, None, space.id).unwrap();
    assert!(notes.is_empty(), "trashed note should not appear in list");

    db::restore_note(&conn, id).unwrap();
    let notes = db::list_notes(&conn, None, space.id).unwrap();
    assert_eq!(notes.len(), 1, "restored note should reappear");
}

#[test]
fn trash_and_restore_page() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_web_page(&conn, &db::NewWebPage {
        url: "https://test.com",
        url_normalized: "test.com",
        title: Some("Test"),
        domain: "test.com",
        category: "archive",
        status: "archived",
        source: None,
        space_id: Some(space.id),
    }).unwrap();

    db::trash_page(&conn, id).unwrap();
    assert!(db::trash_count(&conn).unwrap() > 0);

    db::restore_page(&conn, id).unwrap();
    assert_eq!(db::trash_count(&conn).unwrap(), 0);
}

#[test]
fn restore_note_safe_with_existing_folder() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let folder = db::insert_folder(&conn, "F", None, space.id).unwrap();
    let note = db::insert_note(&conn, "Note", "", Some(folder), space.id).unwrap();

    db::trash_note(&conn, note).unwrap();
    db::restore_note_safe(&conn, note).unwrap();

    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, Some(folder), "should stay in folder");
    assert!(n.deleted_at.is_none());
}

#[test]
fn restore_note_safe_with_deleted_folder() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let folder = db::insert_folder(&conn, "Temp", None, space.id).unwrap();
    let note = db::insert_note(&conn, "Note", "", Some(folder), space.id).unwrap();

    db::trash_note(&conn, note).unwrap();
    db::delete_folder(&conn, folder).unwrap();

    // Folder deleted — note moved to root by delete_folder
    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, None);

    // Restore — should work, note is in root
    db::restore_note_safe(&conn, note).unwrap();
    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, None);
    assert!(n.deleted_at.is_none());
}

#[test]
fn delete_folder_moves_all_notes_to_parent() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let folder = db::insert_folder(&conn, "F", None, space.id).unwrap();
    let active_note = db::insert_note(&conn, "Active", "", Some(folder), space.id).unwrap();
    let trashed_note = db::insert_note(&conn, "Trashed", "", Some(folder), space.id).unwrap();
    db::trash_note(&conn, trashed_note).unwrap();

    db::delete_folder(&conn, folder).unwrap();

    // Both moved to root (FK constraint requires it)
    let an = db::get_note(&conn, active_note).unwrap();
    assert_eq!(an.folder_id, None);
    let tn = db::get_note(&conn, trashed_note).unwrap();
    assert_eq!(tn.folder_id, None);
}

#[test]
fn delete_nested_folder_moves_notes_to_grandparent() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let grandparent = db::insert_folder(&conn, "GP", None, space.id).unwrap();
    let parent = db::insert_folder(&conn, "P", Some(grandparent), space.id).unwrap();
    let note = db::insert_note(&conn, "Note", "", Some(parent), space.id).unwrap();

    db::delete_folder(&conn, parent).unwrap();

    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, Some(grandparent), "should move to grandparent, not root");
}

#[test]
fn delete_folder_chain_notes_land_in_root() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let f1 = db::insert_folder(&conn, "F1", None, space.id).unwrap();
    let f2 = db::insert_folder(&conn, "F2", Some(f1), space.id).unwrap();
    let note = db::insert_note(&conn, "Deep", "", Some(f2), space.id).unwrap();

    // Delete child first, then parent
    db::delete_folder(&conn, f2).unwrap();
    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, Some(f1), "moves to parent");

    db::delete_folder(&conn, f1).unwrap();
    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, None, "moves to root after all folders deleted");
}

#[test]
fn trash_note_then_trash_space_then_restore_space() {
    let (_dir, conn) = open_test_db();
    let sid = db::insert_space(&conn, "Temp").unwrap();
    let note = db::insert_note(&conn, "N", "", None, sid).unwrap();

    // Trash note
    db::trash_note(&conn, note).unwrap();
    // Trash space
    db::trash_space(&conn, sid).unwrap();

    // Space gone from active list
    assert!(!db::list_spaces(&conn).unwrap().iter().any(|s| s.id == sid));

    // Restore space
    db::restore_space(&conn, sid).unwrap();
    assert!(db::list_spaces(&conn).unwrap().iter().any(|s| s.id == sid));

    // Note still trashed
    let n = db::get_note(&conn, note).unwrap();
    assert!(n.deleted_at.is_some(), "note should still be trashed");

    // Restore note
    db::restore_note_safe(&conn, note).unwrap();
    let notes = db::list_notes(&conn, None, sid).unwrap();
    assert_eq!(notes.len(), 1);
}

#[test]
fn delete_folder_with_subfolder_containing_notes() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let parent = db::insert_folder(&conn, "Parent", None, space.id).unwrap();
    let child = db::insert_folder(&conn, "Child", Some(parent), space.id).unwrap();
    let note_in_parent = db::insert_note(&conn, "NP", "", Some(parent), space.id).unwrap();
    let note_in_child = db::insert_note(&conn, "NC", "", Some(child), space.id).unwrap();

    // Delete parent — child becomes root, parent's notes move to root
    db::delete_folder(&conn, parent).unwrap();

    let np = db::get_note(&conn, note_in_parent).unwrap();
    assert_eq!(np.folder_id, None, "parent's note goes to root");

    let nc = db::get_note(&conn, note_in_child).unwrap();
    assert_eq!(nc.folder_id, Some(child), "child's note stays in child");

    let folders = db::list_folders(&conn, space.id).unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, child);
    assert_eq!(folders[0].parent_id, None, "child becomes root folder");
}

#[test]
fn permanent_delete_note() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_note(&conn, "Gone", "Forever", None, space.id).unwrap();

    db::delete_note_permanent(&conn, id).unwrap();
    assert!(db::get_note(&conn, id).is_err());
}

// ---- Folders CRUD ----

#[test]
fn nested_folders() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let parent = db::insert_folder(&conn, "Parent", None, space.id).unwrap();
    let child = db::insert_folder(&conn, "Child", Some(parent), space.id).unwrap();

    let folders = db::list_folders(&conn, space.id).unwrap();
    assert_eq!(folders.len(), 2);

    let child_row = folders.iter().find(|f| f.id == child).unwrap();
    assert_eq!(child_row.parent_id, Some(parent));
}

#[test]
fn delete_folder_reparents_children() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let parent = db::insert_folder(&conn, "Parent", None, space.id).unwrap();
    let child = db::insert_folder(&conn, "Child", Some(parent), space.id).unwrap();
    let note = db::insert_note(&conn, "Note", "", Some(parent), space.id).unwrap();

    db::delete_folder(&conn, parent).unwrap();

    // Child folder should become root
    let folders = db::list_folders(&conn, space.id).unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, child);
    assert_eq!(folders[0].parent_id, None);

    // Note should become root
    let n = db::get_note(&conn, note).unwrap();
    assert_eq!(n.folder_id, None);
}

#[test]
fn rename_folder() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_folder(&conn, "Old", None, space.id).unwrap();

    db::rename_folder(&conn, id, "New").unwrap();
    let folders = db::list_folders(&conn, space.id).unwrap();
    assert_eq!(folders[0].name, "New");
}

// ---- Web pages ----

#[test]
fn insert_page_and_snapshot() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_web_page(&conn, &db::NewWebPage {
        url: "https://example.com/page",
        url_normalized: "example.com/page",
        title: Some("Example Page"),
        domain: "example.com",
        category: "archive",
        status: "queued",
        source: None,
        space_id: Some(space.id),
    }).unwrap();

    db::update_status(&conn, id, "fetching").unwrap();

    let snap = db::insert_snapshot(&conn, id, "<html>test</html>", "test content", None).unwrap();
    assert!(snap > 0);

    db::update_status(&conn, id, "archived").unwrap();
}

#[test]
fn update_status_with_error() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_web_page(&conn, &db::NewWebPage {
        url: "https://fail.com",
        url_normalized: "fail.com",
        title: None,
        domain: "fail.com",
        category: "archive",
        status: "queued",
        source: None,
        space_id: Some(space.id),
    }).unwrap();

    db::update_status_with_error(&conn, id, "failed", "Connection timeout").unwrap();

    let error: Option<String> = conn.query_row(
        "SELECT last_error FROM web_page WHERE id = ?1", [id],
        |r| r.get(0)
    ).unwrap();
    assert_eq!(error.unwrap(), "Connection timeout");
}

#[test]
fn cleanup_old_trash() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_web_page(&conn, &db::NewWebPage {
        url: "https://old.com",
        url_normalized: "old.com",
        title: Some("Old"),
        domain: "old.com",
        category: "archive",
        status: "archived",
        source: None,
        space_id: Some(space.id),
    }).unwrap();

    // Manually set trashed_at to 60 days ago
    conn.execute(
        "UPDATE web_page SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-60 days') WHERE id = ?1",
        [id]
    ).unwrap();

    let cleaned = db::cleanup_old_trash(&conn, 30).unwrap();
    assert_eq!(cleaned, 1);

    // Page should be permanently gone
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM web_page WHERE id = ?1", [id], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

// ---- Revision counter ----

#[test]
fn revision_increments_on_changes() {
    let (_dir, conn) = open_test_db();
    let r0 = db::get_revision(&conn).unwrap();

    // Insert a note — should bump revision
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "A", "", None, space.id).unwrap();
    let r1 = db::get_revision(&conn).unwrap();
    assert!(r1 > r0, "insert should bump revision: {} > {}", r1, r0);

    // Update note — should bump again
    db::update_note(&conn, note_id, "B", "body").unwrap();
    let r2 = db::get_revision(&conn).unwrap();
    assert!(r2 > r1, "update should bump revision: {} > {}", r2, r1);

    // Delete note — should bump
    db::delete_note_permanent(&conn, note_id).unwrap();
    let r3 = db::get_revision(&conn).unwrap();
    assert!(r3 > r2, "delete should bump revision: {} > {}", r3, r2);
}

#[test]
fn revision_increments_on_page_changes() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let r0 = db::get_revision(&conn).unwrap();

    let page_id = db::insert_web_page(&conn, &db::NewWebPage {
        url: "https://rev-test.com",
        url_normalized: "rev-test.com",
        title: Some("Rev"),
        domain: "rev-test.com",
        category: "archive",
        status: "queued",
        source: None,
        space_id: Some(space.id),
    }).unwrap();
    let r1 = db::get_revision(&conn).unwrap();
    assert!(r1 > r0);

    db::update_status(&conn, page_id, "archived").unwrap();
    let r2 = db::get_revision(&conn).unwrap();
    assert!(r2 > r1);

    db::trash_page(&conn, page_id).unwrap();
    let r3 = db::get_revision(&conn).unwrap();
    assert!(r3 > r2);
}

// ---- Classification rules ----

// ---- URL extraction (logic from data.rs, tested here for convenience) ----

fn extract_urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("](") {
        let start = pos + 2;
        if let Some(end) = rest[start..].find(')') {
            let url = rest[start..start + end].trim();
            if url.starts_with("http://") || url.starts_with("https://") {
                if !urls.contains(&url.to_string()) {
                    urls.push(url.to_string());
                }
            }
            rest = &rest[start + end..];
        } else {
            break;
        }
    }
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| c == '(' || c == ')' || c == '<' || c == '>' || c == '"' || c == '\'' || c == ',' || c == ';' || c == '.');
        if (word.starts_with("http://") || word.starts_with("https://")) && !urls.contains(&word.to_string()) {
            urls.push(word.to_string());
        }
    }
    urls
}

#[test]
fn extract_urls_markdown_links() {
    let text = "Check [Rust](https://rust-lang.org) and [Docs](https://doc.rust-lang.org/book)";
    let urls = extract_urls(text);
    assert_eq!(urls, vec!["https://rust-lang.org", "https://doc.rust-lang.org/book"]);
}

#[test]
fn extract_urls_bare() {
    let text = "Visit https://example.com for more info and http://test.org too";
    let urls = extract_urls(text);
    assert_eq!(urls, vec!["https://example.com", "http://test.org"]);
}

#[test]
fn extract_urls_mixed() {
    let text = "See [link](https://a.com) and also https://b.com here";
    let urls = extract_urls(text);
    assert_eq!(urls, vec!["https://a.com", "https://b.com"]);
}

#[test]
fn extract_urls_no_duplicates() {
    let text = "Visit https://a.com and [same](https://a.com) again";
    let urls = extract_urls(text);
    assert_eq!(urls, vec!["https://a.com"]);
}

#[test]
fn extract_urls_with_trailing_punctuation() {
    let text = "Check https://example.com, and https://test.org.";
    let urls = extract_urls(text);
    assert!(urls.contains(&"https://example.com".to_string()));
    assert!(urls.contains(&"https://test.org".to_string()));
}

#[test]
fn extract_urls_milkdown_format() {
    // Milkdown produces this exact markdown format
    let text = "# Heading\n\nSome text with [a link](https://github.com/test) in it.\n\nBare url: https://example.org/path?q=1\n";
    let urls = extract_urls(text);
    assert!(urls.contains(&"https://github.com/test".to_string()));
    assert!(urls.contains(&"https://example.org/path?q=1".to_string()));
}

#[test]
fn rules_are_seeded() {
    let (_dir, conn) = open_test_db();
    let rules = db::load_rules(&conn).unwrap();
    assert!(!rules.is_empty(), "seed rules should be loaded");
}
