//! All user-visible UI strings in one place.
//! Allows easy renaming, localization, and consistency.

// ---- Sidebar ----
pub const NAV_WEBS: &str = "Webs";
pub const NAV_NOTES: &str = "Notes";
pub const NAV_FILES: &str = "Files";
pub const NAV_SEARCH: &str = "Search";
pub const NAV_TRASH: &str = "Trash";
pub const NAV_SETTINGS: &str = "Settings";
pub const DIVIDER_FOLDERS: &str = "Folders";
pub const DIVIDER_SYSTEM: &str = "System";
pub const LABEL_ADD_URL: &str = "Add URL";
pub const PLACEHOLDER_URL: &str = "Paste URL...";

// ---- List panel titles ----
pub const LIST_PAGES: &str = "Pages";
pub const LIST_NOTES: &str = "Notes";
pub const LIST_FILES: &str = "Files";
pub const LIST_TRASH: &str = "Trash";
pub const LIST_SETTINGS: &str = "Settings";

// ---- Settings items ----
pub const SETTINGS_WEBPAGE_RULES: &str = "Webpage rules";
pub const SETTINGS_SPACES: &str = "Spaces";

// ---- Content panel ----
pub const CONTENT_RULES_TITLE: &str = "Classification Rules";
pub const COL_PATTERN: &str = "Pattern";
pub const COL_MATCH_TYPE: &str = "Match type";
pub const COL_CATEGORY: &str = "Category";
pub const COL_NOTE: &str = "Note";

pub const BTN_OPEN_BROWSER: &str = "Open in browser";
pub const BTN_DELETE: &str = "Delete";
pub const BTN_RESTORE: &str = "Restore";
pub const BTN_DELETE_FOREVER: &str = "Delete forever";
pub const BTN_UPLOAD: &str = "Upload";
pub const BTN_SAVE_TO_DOWNLOADS: &str = "Download";

pub const LABEL_CONTENT_PREVIEW: &str = "Content preview";

// ---- Note editor ----
pub const PLACEHOLDER_NOTE_TITLE: &str = "Untitled note";

// ---- Search ----
pub const PLACEHOLDER_SEARCH: &str = "Type to search...";
pub const SEARCH_GROUP_PAGES: &str = "Web Pages";
pub const SEARCH_GROUP_NOTES: &str = "Notes";

// ---- Empty states ----
pub const EMPTY_SELECT: &str = "Select an item to view it here.";
pub const EMPTY_PAGES: &str = "No pages yet. Paste a URL in the sidebar to get started.";
pub const EMPTY_NOTES: &str = "No notes yet. Press Cmd+N to create one.";
pub const EMPTY_FILES: &str = "No files yet. Click Upload to add files.";
pub const EMPTY_FOLDER: &str = "This folder is empty.";
pub const EMPTY_TRASH: &str = "Trash is empty.";
pub const EMPTY_SEARCH: &str = "Type to search across pages and notes.";

pub fn empty_search_no_results(query: &str) -> String {
    format!("No results for \"{}\".", query)
}

// ---- Toast messages ----
pub const TOAST_MOVED_TRASH: &str = "Moved to trash.";
pub const TOAST_NOTE_TRASH: &str = "Note moved to trash.";
pub const TOAST_FILE_TRASH: &str = "File moved to trash.";
pub const TOAST_FILE_SAVED: &str = "File saved.";
pub const TOAST_RESTORED: &str = "Restored.";
pub const TOAST_UNDO: &str = "Undo";
pub const TOAST_FILE_RESTORED_FROM_TRASH: &str = "File restored from trash.";
pub const TOAST_FILE_DEDUPED: &str = "File already exists, using existing.";
pub const TOAST_ATTACHMENT_RESTORED: &str = "Attachment restored.";
pub const TOAST_ATTACHMENT_DEDUPED: &str = "Already attached, linking existing.";
pub const TOAST_ATTACHMENT_REVIVED: &str = "Attachment restored from removed list.";

// ---- Schema mismatch banner (bottom-right, next to revision indicator) ----
pub const SCHEMA_OUTDATED_LABEL: &str = "DB upgraded — restart required";
pub const SCHEMA_OUTDATED_TOOLTIP: &str = "The database schema was updated by another process. \
     This window is still running on the old schema and \
     should be restarted to load the new structure.";

// ---- Trash item labels ----
pub const KIND_PAGE: &str = "page";
pub const KIND_NOTE: &str = "note";
pub const KIND_FILE: &str = "file";

// ---- Metadata ----
pub const NO_TITLE: &str = "(no title)";
pub const LABEL_ERROR: &str = "Error";
pub const BTN_RETRY: &str = "Retry";
pub const BTN_MOVE_TO: &str = "Move to...";
pub const MOVE_TO_ROOT: &str = "Notes (root)";

// ---- Web archive — versions ----
pub const BTN_REACHIVE: &str = "Re-archive";
pub const LABEL_VERSIONS: &str = "Versions";
pub const BADGE_CURRENT: &str = "current";
pub const BADGE_TITLE_CHANGED: &str = "title changed";
pub const BADGE_NO_CHANGE: &str = "no change";
pub const BTN_DELETE_VERSION: &str = "Delete this version";
pub const TOAST_REACHIVE_QUEUED: &str = "Queued for re-archive.";
pub const TOAST_VERSION_DELETED: &str = "Snapshot version deleted.";
