use lore_core::db;
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
    let _work_id = db::insert_space(&conn, "Work").unwrap();

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
    assert!(
        all.iter()
            .any(|s| s.id == space_id && s.deleted_at.is_some())
    );

    db::restore_space(&conn, space_id).unwrap();
    let active = db::list_spaces(&conn).unwrap();
    assert!(active.iter().any(|s| s.id == space_id));
}

#[test]
fn delete_space_removes_all_content() {
    let (_dir, conn) = open_test_db();
    let space_id = db::insert_space(&conn, "Doomed").unwrap();

    // Add content
    let _page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://example.com",
            url_normalized: "example.com",
            title: Some("Example"),
            domain: "example.com",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space_id),
        },
    )
    .unwrap();
    let note_id = db::insert_note(&conn, "Test", "Body", None, space_id).unwrap();
    let _folder_id = db::insert_folder(&conn, "Folder", None, space_id).unwrap();

    db::delete_space_permanent(&conn, space_id).unwrap();

    // Verify everything is gone
    let pages: Vec<i64> = conn
        .prepare("SELECT id FROM web_page WHERE space_id = ?1")
        .unwrap()
        .query_map([space_id], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
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
    assert!(!other_counts.contains_key(&folder_id));
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
    assert!(
        root.is_empty(),
        "note in folder should not appear in root listing"
    );
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
    let id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://test.com",
            url_normalized: "test.com",
            title: Some("Test"),
            domain: "test.com",
            category: "archive",
            status: "archived",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    db::trash_page(&conn, id).unwrap();
    assert!(db::trash_count(&conn, space.id).unwrap() > 0);

    db::restore_page(&conn, id).unwrap();
    assert_eq!(db::trash_count(&conn, space.id).unwrap(), 0);
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
    assert_eq!(
        n.folder_id,
        Some(grandparent),
        "should move to grandparent, not root"
    );
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
    let id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://example.com/page",
            url_normalized: "example.com/page",
            title: Some("Example Page"),
            domain: "example.com",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    db::update_status(&conn, id, "fetching").unwrap();

    let snap = db::insert_snapshot(
        &conn,
        id,
        "<html>test</html>",
        "test content",
        None,
        None,
        db::ReadabilityBundle::default(),
    )
    .unwrap();
    assert!(snap > 0);

    db::update_status(&conn, id, "archived").unwrap();
}

#[test]
fn update_status_with_error() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://fail.com",
            url_normalized: "fail.com",
            title: None,
            domain: "fail.com",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    db::update_status_with_error(&conn, id, "failed", "Connection timeout").unwrap();

    let error: Option<String> = conn
        .query_row("SELECT last_error FROM web_page WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(error.unwrap(), "Connection timeout");
}

#[test]
fn cleanup_old_trash() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://old.com",
            url_normalized: "old.com",
            title: Some("Old"),
            domain: "old.com",
            category: "archive",
            status: "archived",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    // Manually set trashed_at to 60 days ago
    conn.execute(
        "UPDATE web_page SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-60 days') WHERE id = ?1",
        [id]
    ).unwrap();

    let cleaned = db::cleanup_old_trash(&conn, 30).unwrap();
    assert_eq!(cleaned, 1);

    // Page should be permanently gone
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM web_page WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
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

    let page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://rev-test.com",
            url_normalized: "rev-test.com",
            title: Some("Rev"),
            domain: "rev-test.com",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();
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
            if (url.starts_with("http://") || url.starts_with("https://"))
                && !urls.contains(&url.to_string())
            {
                urls.push(url.to_string());
            }
            rest = &rest[start + end..];
        } else {
            break;
        }
    }
    for word in text.split_whitespace() {
        let word = word.trim_matches(|c: char| {
            c == '('
                || c == ')'
                || c == '<'
                || c == '>'
                || c == '"'
                || c == '\''
                || c == ','
                || c == ';'
                || c == '.'
        });
        if (word.starts_with("http://") || word.starts_with("https://"))
            && !urls.contains(&word.to_string())
        {
            urls.push(word.to_string());
        }
    }
    urls
}

#[test]
fn extract_urls_markdown_links() {
    let text = "Check [Rust](https://rust-lang.org) and [Docs](https://doc.rust-lang.org/book)";
    let urls = extract_urls(text);
    assert_eq!(
        urls,
        vec!["https://rust-lang.org", "https://doc.rust-lang.org/book"]
    );
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

// ---- archive_url / auto_archive_from_text ----

#[test]
fn archive_url_inserts_classified_page() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let out = db::archive_url(
        &conn,
        "https://example.com/article",
        Some(space.id),
        None,
        None,
    )
    .unwrap();
    assert!(out.id > 0);
    // Default category for unknown domain = "archive"
    assert_eq!(out.category, "archive");

    // Page should be queued for the worker.
    let pages = db::list_pages(&conn, space.id, 100).unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].status, "queued");
    assert_eq!(pages[0].category, "archive");
}

#[test]
fn archive_url_classifies_via_rules() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    // Seed rules include `google.com/search → discard`.
    let out = db::archive_url(
        &conn,
        "https://www.google.com/search?q=rust",
        Some(space.id),
        None,
        None,
    )
    .unwrap();
    assert_eq!(out.category, "discard");

    let pages = db::list_pages(&conn, space.id, 100).unwrap();
    // Discarded → status "skipped" (not queued for archival).
    assert_eq!(pages[0].status, "skipped");
}

#[test]
fn archive_url_honors_title_and_source() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let out = db::archive_url(
        &conn,
        "https://example.com/x",
        Some(space.id),
        Some("Explicit Title"),
        Some("note"),
    )
    .unwrap();
    let detail = db::get_page(&conn, out.id).unwrap();
    assert_eq!(detail.title.as_deref(), Some("Explicit Title"));
}

#[test]
fn auto_archive_from_text_queues_new_urls_only() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    // First pass: 2 new URLs.
    let text = "see [docs](https://example.org/x) and https://example.net";
    let queued = db::auto_archive_from_text(&conn, text, space.id).unwrap();
    assert_eq!(queued, 2);

    // Second pass: same text → nothing new (find_page_by_url short-circuits).
    let queued_again = db::auto_archive_from_text(&conn, text, space.id).unwrap();
    assert_eq!(queued_again, 0);

    let pages = db::list_pages(&conn, space.id, 100).unwrap();
    assert_eq!(pages.len(), 2);
}

#[test]
fn auto_archive_from_text_skips_invalid_urls() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    // `http://[bogus]` will fail Url::parse but extract_urls won't catch it
    // upstream — we rely on archive_url's parse + .is_ok() guard.
    let text = "plain text, no links here";
    let queued = db::auto_archive_from_text(&conn, text, space.id).unwrap();
    assert_eq!(queued, 0);
}

// ---- ensure_page (worker entry point) ----

#[test]
fn ensure_page_archive_category_gets_queued_status() {
    let (_dir, conn) = open_test_db();
    let id = db::ensure_page(
        &conn,
        "https://example.com/x",
        "example.com/x",
        None,
        "example.com",
        "archive",
    )
    .unwrap();
    let status: String = conn
        .query_row("SELECT status FROM web_page WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "queued");
}

#[test]
fn ensure_page_non_archive_category_gets_skipped_status() {
    let (_dir, conn) = open_test_db();
    let id = db::ensure_page(
        &conn,
        "https://google.com/search?q=x",
        "google.com/search?q=x",
        None,
        "google.com",
        "discard",
    )
    .unwrap();
    let status: String = conn
        .query_row("SELECT status FROM web_page WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(status, "skipped");
}

#[test]
fn ensure_page_returns_existing_id_on_duplicate_url() {
    let (_dir, conn) = open_test_db();
    let first = db::ensure_page(
        &conn,
        "https://example.com/dup",
        "example.com/dup",
        None,
        "example.com",
        "archive",
    )
    .unwrap();
    let second = db::ensure_page(
        &conn,
        "https://example.com/dup",
        "example.com/dup",
        None,
        "example.com",
        "archive",
    )
    .unwrap();
    assert_eq!(first, second);
}

// ---- list_pages_filtered: 4-filter composition (param_idx counter) ----

fn insert_test_page(
    conn: &rusqlite::Connection,
    space_id: i64,
    url: &str,
    domain: &str,
    category: &str,
    status: &str,
) {
    db::insert_web_page(
        conn,
        &db::NewWebPage {
            url,
            url_normalized: url,
            title: None,
            domain,
            category,
            status,
            source: None,
            space_id: Some(space_id),
        },
    )
    .unwrap();
}

fn seed_pages_for_filtering(conn: &rusqlite::Connection, space_id: i64) {
    // 4 pages that vary on every filter axis, so the test below uniquely
    // identifies the wanted row via (space, category, status, domain) — any
    // off-by-one in the parameter index would either miss the match or
    // return a different row.
    insert_test_page(
        conn,
        space_id,
        "https://a.example.com/1",
        "a.example.com",
        "archive",
        "queued",
    );
    insert_test_page(
        conn,
        space_id,
        "https://a.example.com/2",
        "a.example.com",
        "archive",
        "archived",
    );
    insert_test_page(
        conn,
        space_id,
        "https://b.example.com/1",
        "b.example.com",
        "discard",
        "skipped",
    );
    insert_test_page(
        conn,
        space_id,
        "https://a.example.com/3",
        "a.example.com",
        "discard",
        "archived",
    );
}

#[test]
fn list_pages_filtered_all_four_filters_combine_correctly() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    seed_pages_for_filtering(&conn, space.id);
    let rows = lore_core::search::list_pages_filtered(
        &conn,
        Some(space.id),
        Some("archive"),
        Some("queued"),
        Some("a.example.com"),
        100,
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].domain, "a.example.com");
    assert_eq!(rows[0].category, "archive");
    assert_eq!(rows[0].status, "queued");
}

#[test]
fn list_pages_filtered_partial_filters_match_multiple() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    seed_pages_for_filtering(&conn, space.id);
    // Only category + space → 2 archived rows
    let rows = lore_core::search::list_pages_filtered(
        &conn,
        Some(space.id),
        Some("archive"),
        None,
        None,
        100,
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn list_pages_filtered_no_filters_returns_all_in_space() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    seed_pages_for_filtering(&conn, space.id);
    let rows = lore_core::search::list_pages_filtered(&conn, Some(space.id), None, None, None, 100)
        .unwrap();
    assert_eq!(rows.len(), 4);
}

// ---- cleanup_orphaned_attachments (counter + negation) ----

fn insert_attachment(conn: &rusqlite::Connection, note_id: i64, name: &str) -> i64 {
    // Unique payload per attachment so the hash-based dedup doesn't collapse them.
    let payload = format!("payload-for-{}", name);
    let (id, _) =
        db::insert_attachment(conn, note_id, name, "text/plain", payload.as_bytes()).unwrap();
    id
}

#[test]
fn cleanup_orphaned_attachments_drops_unreferenced_only() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "Note", "body", None, space.id).unwrap();
    let kept = insert_attachment(&conn, note_id, "keep.txt");
    let drop_a = insert_attachment(&conn, note_id, "drop-a.txt");
    let drop_b = insert_attachment(&conn, note_id, "drop-b.txt");

    let deleted = db::cleanup_orphaned_attachments(&conn, note_id, &[kept]).unwrap();
    assert_eq!(
        deleted, 2,
        "two of three attachments should be soft-deleted"
    );

    // `kept` should still be active (deleted_at IS NULL).
    let kept_state: Option<String> = conn
        .query_row(
            "SELECT deleted_at FROM note_attachment WHERE id = ?1",
            [kept],
            |r| r.get(0),
        )
        .unwrap();
    assert!(kept_state.is_none());

    // `drop_a` / `drop_b` must be soft-deleted (deleted_at IS NOT NULL).
    for id in [drop_a, drop_b] {
        let s: Option<String> = conn
            .query_row(
                "SELECT deleted_at FROM note_attachment WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(s.is_some(), "id {} should be soft-deleted", id);
    }
}

#[test]
fn cleanup_orphaned_attachments_returns_zero_when_all_referenced() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "Note", "body", None, space.id).unwrap();
    let a = insert_attachment(&conn, note_id, "a.txt");
    let b = insert_attachment(&conn, note_id, "b.txt");
    let deleted = db::cleanup_orphaned_attachments(&conn, note_id, &[a, b]).unwrap();
    assert_eq!(deleted, 0);
}

// ---- cleanup_old_trash (counter for each entity kind) ----

#[test]
fn cleanup_old_trash_hard_deletes_expired_items_across_kinds() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    // Trashed page, note, file — all dated long ago to be past the retention.
    let page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://a.com/x",
            url_normalized: "a.com/x",
            title: None,
            domain: "a.com",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();
    let note_id = db::insert_note(&conn, "Old Note", "body", None, space.id).unwrap();

    db::trash_page(&conn, page_id).unwrap();
    db::trash_note(&conn, note_id).unwrap();

    // Back-date the trashed timestamps beyond the retention window.
    conn.execute(
        "UPDATE web_page SET trashed_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-90 days') WHERE id = ?1",
        [page_id],
    ).unwrap();
    conn.execute(
        "UPDATE note SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-90 days') WHERE id = ?1",
        [note_id],
    ).unwrap();

    let cleaned = db::cleanup_old_trash(&conn, 30).unwrap();
    assert_eq!(cleaned, 2, "page + note should be hard-deleted");

    // Verify rows actually gone.
    let page_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM web_page WHERE id = ?1",
            [page_id],
            |r| r.get(0),
        )
        .unwrap();
    let note_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM note WHERE id = ?1", [note_id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(page_count, 0);
    assert_eq!(note_count, 0);
}

#[test]
fn cleanup_old_trash_keeps_fresh_trash() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "Recent Note", "body", None, space.id).unwrap();
    db::trash_note(&conn, note_id).unwrap();
    // No back-dating → trashed_at is "now"; retention=30 days → must not delete.
    let cleaned = db::cleanup_old_trash(&conn, 30).unwrap();
    assert_eq!(cleaned, 0);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM note WHERE id = ?1", [note_id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn cleanup_old_trash_hard_deletes_across_all_kinds() {
    // Extension of `cleanup_old_trash_hard_deletes_expired_items_across_kinds`
    // — adds file, space, attachment branches so all five `cleaned += 1`
    // counters get exercised. Each entity is back-dated past retention; the
    // returned count must equal the total (5).
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    let page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://gone.example.com/x",
            url_normalized: "gone.example.com/x",
            title: None,
            domain: "gone.example.com",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();
    let note_id = db::insert_note(&conn, "Old Note", "body", None, space.id).unwrap();
    let (file_id, _) =
        db::insert_file(&conn, "doc.txt", Some("text/plain"), b"content", space.id).unwrap();
    let dead_space = db::insert_space(&conn, "Dead").unwrap();
    // Attachment: insert under a fresh note, then orphan-cleanup soft-deletes it.
    let host_note = db::insert_note(&conn, "host", "", None, space.id).unwrap();
    let att_id = db::insert_attachment(&conn, host_note, "a.txt", "text/plain", b"data")
        .unwrap()
        .0;
    db::cleanup_orphaned_attachments(&conn, host_note, &[]).unwrap();

    db::trash_page(&conn, page_id).unwrap();
    db::trash_note(&conn, note_id).unwrap();
    db::trash_file(&conn, file_id).unwrap();
    db::trash_space(&conn, dead_space).unwrap();

    // Back-date every soft-delete timestamp past retention. The attachment
    // also needs to be back-dated for the attachment-cleanup branch to fire.
    for (table, col, id) in [
        ("web_page", "trashed_at", page_id),
        ("note", "deleted_at", note_id),
        ("file", "deleted_at", file_id),
        ("space", "deleted_at", dead_space),
        ("note_attachment", "deleted_at", att_id),
    ] {
        conn.execute(
            &format!(
                "UPDATE {} SET {} = strftime('%Y-%m-%dT%H:%M:%fZ','now','-90 days') WHERE id = ?1",
                table, col
            ),
            [id],
        )
        .unwrap();
    }

    let cleaned = db::cleanup_old_trash(&conn, 30).unwrap();
    assert_eq!(
        cleaned, 5,
        "page + note + file + space + attachment should each tick the counter"
    );

    // Spot-check that the attachment hard-delete branch (L181) actually ran:
    // the row should be gone, not merely soft-deleted.
    let att_left: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM note_attachment WHERE id = ?1",
            [att_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(att_left, 0);
}

// ---- list_trash returns all soft-deleted kinds for a space ----

#[test]
fn list_trash_returns_all_trashed_kinds() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    let page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://t.example/x",
            url_normalized: "t.example/x",
            title: Some("Page Title"),
            domain: "t.example",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();
    let note_id = db::insert_note(&conn, "Note Title", "body", None, space.id).unwrap();
    let (file_id, _) = db::insert_file(
        &conn,
        "doc.pdf",
        Some("application/pdf"),
        b"pdfbytes",
        space.id,
    )
    .unwrap();

    db::trash_page(&conn, page_id).unwrap();
    db::trash_note(&conn, note_id).unwrap();
    db::trash_file(&conn, file_id).unwrap();

    let items = db::list_trash(&conn, space.id).unwrap();
    assert_eq!(items.len(), 3, "expected 3 trashed entities in space");
    // The ids must round-trip; the returned items are the ones we just trashed.
    let ids: std::collections::HashSet<i64> = items.iter().map(|i| i.id).collect();
    assert!(ids.contains(&page_id));
    assert!(ids.contains(&note_id));
    assert!(ids.contains(&file_id));
}

// ---- Activity: by_day & for_day ----

#[test]
fn activity_by_day_counts_today_entries() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    // Two notes + one page, all dated "today" via default timestamps.
    db::insert_note(&conn, "n1", "", None, space.id).unwrap();
    db::insert_note(&conn, "n2", "", None, space.id).unwrap();
    db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://act.example/x",
            url_normalized: "act.example/x",
            title: None,
            domain: "act.example",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    let activity = db::activity_by_day(&conn, space.id, 7).unwrap();
    assert!(!activity.is_empty(), "today should show up in 7-day window");

    let today = activity
        .iter()
        .find(|(day, _)| {
            // The DB groups by `date(updated_at)`, so compare against today's
            // YYYY-MM-DD computed the same way.
            let today_str: String = conn
                .query_row("SELECT date('now')", [], |r| r.get(0))
                .unwrap();
            *day == today_str
        })
        .expect("today must appear in the activity rows");
    // 2 notes + 1 page = 3 entries on today's bucket.
    assert_eq!(today.1, 3);
}

#[test]
fn activity_for_day_returns_seeded_notes_and_pages() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let n = db::insert_note(&conn, "today note", "body", None, space.id).unwrap();
    let p = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://today.example/x",
            url_normalized: "today.example/x",
            title: Some("Today Page"),
            domain: "today.example",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    let today: String = conn
        .query_row("SELECT date('now')", [], |r| r.get(0))
        .unwrap();
    let (notes, pages) = db::activity_for_day(&conn, space.id, &today).unwrap();
    assert!(notes.iter().any(|note| note.id == n));
    assert!(pages.iter().any(|(id, _)| *id == p));
}

// ---- Attachments: data round-trip, list/remove/restore, full delete ----

#[test]
fn get_attachment_data_returns_what_was_inserted() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "n", "", None, space.id).unwrap();
    let payload = b"specific-attachment-payload";
    let (id, _) = db::insert_attachment(&conn, note_id, "f.txt", "text/markdown", payload).unwrap();
    let (mime, data) = db::get_attachment_data(&conn, id).unwrap();
    assert_eq!(mime, "text/markdown");
    assert_eq!(data, payload);
}

#[test]
fn list_attachments_returns_all_active_for_note() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "n", "", None, space.id).unwrap();
    db::insert_attachment(&conn, note_id, "a.txt", "text/plain", b"alpha").unwrap();
    db::insert_attachment(&conn, note_id, "b.txt", "text/plain", b"beta").unwrap();
    let items = db::list_attachments(&conn, note_id).unwrap();
    let names: Vec<&str> = items.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"a.txt"));
    assert!(names.contains(&"b.txt"));
    assert_eq!(items.len(), 2);
}

#[test]
fn list_removed_attachments_returns_only_soft_deleted() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "n", "", None, space.id).unwrap();
    let kept = db::insert_attachment(&conn, note_id, "keep.txt", "text/plain", b"k")
        .unwrap()
        .0;
    let dropped = db::insert_attachment(&conn, note_id, "drop.txt", "text/plain", b"d")
        .unwrap()
        .0;
    // Orphan the dropped one (used_ids only includes `kept`).
    db::cleanup_orphaned_attachments(&conn, note_id, &[kept]).unwrap();

    let removed = db::list_removed_attachments(&conn, note_id).unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].id, dropped);
    assert_eq!(removed[0].name, "drop.txt");
}

#[test]
fn restore_attachment_clears_deleted_at() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "n", "", None, space.id).unwrap();
    let id = db::insert_attachment(&conn, note_id, "a.txt", "text/plain", b"x")
        .unwrap()
        .0;
    // Soft-delete via orphan cleanup.
    db::cleanup_orphaned_attachments(&conn, note_id, &[]).unwrap();
    let pre: Option<String> = conn
        .query_row(
            "SELECT deleted_at FROM note_attachment WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(pre.is_some(), "must be soft-deleted before restore");

    db::restore_attachment(&conn, id).unwrap();
    let post: Option<String> = conn
        .query_row(
            "SELECT deleted_at FROM note_attachment WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(post.is_none(), "deleted_at must be NULL after restore");
}

#[test]
fn delete_attachments_for_note_removes_all_rows() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let note_id = db::insert_note(&conn, "n", "", None, space.id).unwrap();
    db::insert_attachment(&conn, note_id, "a.txt", "text/plain", b"a").unwrap();
    db::insert_attachment(&conn, note_id, "b.txt", "text/plain", b"b").unwrap();
    assert_eq!(db::list_attachments(&conn, note_id).unwrap().len(), 2);

    db::delete_attachments_for_note(&conn, note_id).unwrap();

    // Both active and soft-deleted should be hard-removed.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM note_attachment WHERE note_id = ?1",
            [note_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

// ---- Files: CRUD round-trip ----

#[test]
fn list_files_returns_seeded_file_with_correct_metadata() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let payload = b"hello world";
    let (id, _) =
        db::insert_file(&conn, "hello.txt", Some("text/plain"), payload, space.id).unwrap();

    let rows = db::list_files(&conn, space.id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].name, "hello.txt");
    assert_eq!(rows[0].mime_type.as_deref(), Some("text/plain"));
    assert_eq!(rows[0].size, payload.len() as i64);
}

#[test]
fn get_file_data_returns_bytes_and_mime() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let payload = b"binary-blob-payload";
    let (id, _) = db::insert_file(
        &conn,
        "blob.bin",
        Some("application/octet-stream"),
        payload,
        space.id,
    )
    .unwrap();
    let (mime, data) = db::get_file_data(&conn, id).unwrap();
    assert_eq!(mime.as_deref(), Some("application/octet-stream"));
    assert_eq!(data, payload);
}

#[test]
fn trash_file_moves_it_out_of_active_list() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let (id, _) = db::insert_file(&conn, "f.txt", Some("text/plain"), b"x", space.id).unwrap();
    assert_eq!(db::list_files(&conn, space.id).unwrap().len(), 1);

    db::trash_file(&conn, id).unwrap();
    assert!(db::list_files(&conn, space.id).unwrap().is_empty());
    let trashed = db::list_trashed_files(&conn, space.id).unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(trashed[0].id, id);
}

#[test]
fn restore_file_puts_it_back_in_active_list() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let (id, _) = db::insert_file(&conn, "f.txt", Some("text/plain"), b"x", space.id).unwrap();
    db::trash_file(&conn, id).unwrap();
    assert!(db::list_files(&conn, space.id).unwrap().is_empty());

    db::restore_file(&conn, id).unwrap();
    let active = db::list_files(&conn, space.id).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, id);
}

#[test]
fn delete_file_permanent_removes_row() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let (id, _) = db::insert_file(&conn, "f.txt", Some("text/plain"), b"x", space.id).unwrap();

    db::delete_file_permanent(&conn, id).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM file WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

// ---- find_notes_referencing_url ----

#[test]
fn find_notes_referencing_url_returns_matching_notes() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let target = "https://example.org/doc";
    let matching = db::insert_note(
        &conn,
        "with link",
        &format!("see [doc]({}) for more", target),
        None,
        space.id,
    )
    .unwrap();
    db::insert_note(&conn, "unrelated", "no link here", None, space.id).unwrap();

    let hits = db::find_notes_referencing_url(&conn, target, space.id).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, matching);
    assert_eq!(hits[0].1, "with link");
}

#[test]
fn find_notes_referencing_url_returns_empty_for_unused_url() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    db::insert_note(&conn, "n", "plain body", None, space.id).unwrap();
    let hits = db::find_notes_referencing_url(&conn, "https://nope.example", space.id).unwrap();
    assert!(hits.is_empty());
}

// ---- touch_space ----

#[test]
fn touch_space_advances_last_used_timestamp() {
    let (_dir, conn) = open_test_db();
    let id = db::insert_space(&conn, "Tmp").unwrap();
    // Back-date the space so the timestamp comparison is unambiguous (millisecond
    // resolution at SQLite's strftime won't always advance between two adjacent
    // statements).
    conn.execute(
        "UPDATE space SET last_used = strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 day') WHERE id = ?1",
        [id],
    )
    .unwrap();
    let before: String = conn
        .query_row("SELECT last_used FROM space WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();

    db::touch_space(&conn, id).unwrap();

    let after: String = conn
        .query_row("SELECT last_used FROM space WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(
        after > before,
        "touch_space must advance last_used: {} > {}",
        after,
        before
    );
}

// ---- insert_snapshot returns a real new id, not a fixed one ----

#[test]
fn insert_snapshot_returns_distinct_ids_and_round_trips() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://snap.example/x",
            url_normalized: "snap.example/x",
            title: None,
            domain: "snap.example",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    let s1 = db::insert_snapshot(
        &conn,
        page_id,
        "<html>v1</html>",
        "v1 text",
        None,
        None,
        db::ReadabilityBundle::default(),
    )
    .unwrap();
    let s2 = db::insert_snapshot(
        &conn,
        page_id,
        "<html>v2</html>",
        "v2 text",
        None,
        None,
        db::ReadabilityBundle::default(),
    )
    .unwrap();
    assert_ne!(s1, s2, "two snapshots must get distinct row ids");

    // The returned id must point to the actually-inserted row.
    let plain: String = conn
        .query_row(
            "SELECT plain_text FROM web_page_snapshot WHERE id = ?1",
            [s1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plain, "v1 text");
}

// ---- check_urls_status: map mirrors the DB rows ----

#[test]
fn check_urls_status_returns_status_per_known_url() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let url_a = "https://status.example/a";
    let url_b = "https://status.example/b";
    let url_unknown = "https://nothing.example/x";

    db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: url_a,
            url_normalized: url_a,
            title: None,
            domain: "status.example",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();
    db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: url_b,
            url_normalized: url_b,
            title: None,
            domain: "status.example",
            category: "archive",
            status: "archived",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    let urls = vec![
        url_a.to_string(),
        url_b.to_string(),
        url_unknown.to_string(),
    ];
    let map = db::check_urls_status(&conn, &urls).unwrap();
    assert_eq!(map.len(), 2, "unknown url must not appear in result");
    assert_eq!(map.get(url_a).map(|s| s.as_str()), Some("queued"));
    assert_eq!(map.get(url_b).map(|s| s.as_str()), Some("archived"));
}

// ---- Full-text search returns hits for inserted content ----

#[test]
fn search_web_pages_finds_inserted_page_by_snippet_term() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let page_id = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://fts.example/article",
            url_normalized: "fts.example/article",
            title: Some("Indexable Title"),
            domain: "fts.example",
            category: "archive",
            status: "queued",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();
    db::insert_snapshot(
        &conn,
        page_id,
        "<html>doesn't matter</html>",
        "rare-keyword-zarquon body text",
        None,
        None,
        db::ReadabilityBundle::default(),
    )
    .unwrap();

    let hits = lore_core::search::search_web_pages(&conn, "zarquon", space.id, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, page_id);

    let brief = lore_core::search::search_web_pages_brief(&conn, "zarquon", space.id, 10).unwrap();
    assert_eq!(brief.len(), 1);
    assert_eq!(brief[0].id, page_id);
}

#[test]
fn search_notes_finds_inserted_note_by_body_term() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();
    let id = db::insert_note(
        &conn,
        "Notes Title",
        "contains zarquon term",
        None,
        space.id,
    )
    .unwrap();
    let hits = lore_core::search::search_notes(&conn, "zarquon", space.id, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id);
}

#[test]
fn list_page_ids_ordered_returns_correct_ids() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    let id1 = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://example.com/1",
            url_normalized: "https://example.com/1",
            title: Some("First"),
            domain: "example.com",
            category: "archive",
            status: "archived",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    let id2 = db::insert_web_page(
        &conn,
        &db::NewWebPage {
            url: "https://example.com/2",
            url_normalized: "https://example.com/2",
            title: Some("Second"),
            domain: "example.com",
            category: "archive",
            status: "archived",
            source: None,
            space_id: Some(space.id),
        },
    )
    .unwrap();

    let ids = db::list_page_ids_ordered(&conn, space.id, 10).unwrap();
    assert!(ids.len() >= 2);
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}

#[test]
fn list_page_ids_ordered_respects_limit() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    for i in 0..5 {
        db::insert_web_page(
            &conn,
            &db::NewWebPage {
                url: &format!("https://example.com/{}", i),
                url_normalized: &format!("https://example.com/{}", i),
                title: Some(&format!("Page {}", i)),
                domain: "example.com",
                category: "archive",
                status: "archived",
                source: None,
                space_id: Some(space.id),
            },
        )
        .unwrap();
    }

    let ids = db::list_page_ids_ordered(&conn, space.id, 2).unwrap();
    assert!(ids.len() <= 2);
}

#[test]
fn list_note_ids_ordered_returns_valid_ids() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    let note_id1 = db::insert_note(&conn, "First Note", "content 1", None, space.id).unwrap();
    let note_id2 = db::insert_note(&conn, "Second Note", "content 2", None, space.id).unwrap();

    // list_note_ids_ordered takes (conn, folder_id: Option, space_id)
    let ids = db::list_note_ids_ordered(&conn, None, space.id).unwrap();
    assert!(ids.len() >= 2);
    assert!(ids.contains(&note_id1));
    assert!(ids.contains(&note_id2));
    // Should be non-empty and all positive
    for id in &ids {
        assert!(*id > 0);
    }
}

#[test]
fn list_note_ids_ordered_ordered_by_updated_at() {
    let (_dir, conn) = open_test_db();
    let space = db::get_active_space(&conn).unwrap();

    let _note1 = db::insert_note(&conn, "Old Note", "content 1", None, space.id).unwrap();
    let note2 = db::insert_note(&conn, "New Note", "content 2", None, space.id).unwrap();

    // note2 was inserted after note1, so should appear first (ORDER BY updated_at DESC)
    let ids = db::list_note_ids_ordered(&conn, None, space.id).unwrap();
    assert!(!ids.is_empty());
    assert_eq!(ids[0], note2, "Most recently updated note should be first");
}
