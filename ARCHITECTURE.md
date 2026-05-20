# Architecture

## Crate overview

```
crates/
├── lore-core/    Pure library. DB types, SQL, migrations, FTS5 search,
│                 URL classification. No network, no Dioxus. WASM-safe.
├── lore-ui/      Dioxus app — desktop (native WebView) + web (WASM).
│                 Two cargo features: `desktop` (default) / `web`.
├── lore-server/  axum HTTP server. Exposes lore-core over JSON RPC.
│                 Serves the WASM bundle as static files.
├── lore-worker/  Headless Chrome archiver. Reads/writes SQLite directly.
├── lore-cli/     Command-line interface. Reads/writes SQLite directly.
└── lore-e2e/     Integration tests. Spawns lore-server + drives browser.
```

### Dependency graph (enforced by `.sentrux/rules.toml`)

```
lore-ui     ┐
lore-server ├──► lore-core   (one-way; binaries are peers, never depend on each other)
lore-worker ┤
lore-cli    ┘
```

The four binaries communicate exclusively through the SQLite file — never
through function calls, shared memory, or sockets between themselves.

---

## Client entry points

### Desktop app (`lore-ui`, feature `desktop`)

`main()` in `lore-ui/src/main.rs`:

1. Compute DB path (`data::db_path` — `~/.local/share/lore/lore.db` on Linux,
   `~/Library/Application Support/lore/lore.db` on macOS).
2. `lore_core::db::open(&db_path)` — bootstrap: apply pending migrations, seed
   default space + classification rules, refuse if DB is from a newer build.
3. Install `LocalBackend` as the process-wide backend (`backend::init`).
4. Render `BootedApp`. On bootstrap failure render `StartupError` overlay
   instead of a blank window.

All subsequent data access goes through `backend::current()` — an
`Arc<dyn Backend>` stored in a `OnceLock`.

### Web UI (`lore-ui`, feature `web`)

Same `app()` component tree as desktop; only the startup sequence differs:

1. Browser loads `index.html` from `lore-server`.
2. Dioxus WASM runtime calls `app()`.
3. Install `HttpBackend::new("/api")` as the process-wide backend.
4. Render `BootedApp`. (No migration step — the server already bootstrapped.)

All data calls become HTTP `POST /api/<method>` requests to the same origin.

### CLI (`lore-cli`)

Each command opens its own connection with `db::open(&db_path)` (bootstrap
path) and calls `lore_core::db::*` and `lore_core::search::*` directly.
No backend trait, no async, no Dioxus.

```
lore add <url>        → db::archive_url
lore search <query>   → search::search_web_pages
lore list             → search::list_pages_filtered
lore db-version       → migrations::current_version (raw, no migration)
lore migrate          → db::open (migration as side effect)
```

### Archive worker (`lore-worker`)

Single-threaded, no HTTP, no async runtime:

1. `db::open(&db_path)` — bootstrap.
2. `archive::archive_url` — archive one specific URL, or
   `archive::archive_queued` — drain the queue (up to `--limit` items).

Communicates with the UI and server exclusively through the SQLite file —
the UI writes `status = 'pending'` rows; the worker reads them, fetches with
headless Chrome, writes `status = 'archived'` + snapshot HTML back.

---

## The Backend trait

`lore-ui/src/backend/mod.rs` defines the central abstraction:

```rust
#[async_trait(?Send)]
pub trait Backend: Send + Sync {
    // 50+ methods covering every domain
}
```

**`LocalBackend`** (`backend/local.rs`, feature `desktop`):  
Wraps synchronous `lore_core::db::*` calls in `async {}` blocks. Each method
call opens a fresh connection via `db::open_existing` (no migration overhead).
Futures resolve immediately — there is no real async work.

**`HttpBackend`** (`backend/http.rs`, feature `web`):  
Each method serializes its parameters to JSON and POSTs to
`/api/<method_name>` via `gloo-net`. On non-2xx, the body is parsed as
`BackendError` so the error code survives the round trip unchanged.

The swap is a one-liner in `app()`:

```rust
// Desktop:
backend::init(Arc::new(LocalBackend::new(db_path)));

// Web:
backend::init(Arc::new(HttpBackend::new("/api".to_string())));
```

---

## Call path: a single mutation

Using "create note" (Cmd+N) as the canonical example:

```
Desktop                             Web

Keypress → handle_keyboard          Keypress → handle_keyboard
  create_new_note(state, store)       create_new_note(state, store)
    spawn(async move {                  spawn(async move {
      store.create_note(&state)           store.create_note(&state)
        backend::current()                  backend::current()
          .create_note(...)                   .create_note(...)
                │                                     │
          LocalBackend                          HttpBackend
                │                                     │
     db::insert_note(&conn, ...)        POST /api/create_note  ←── lore-server
                │                                     │
         SQLite write                         handlers::create_note
                │                               db::insert_note(&conn, ...)
                                                        │
                                                  SQLite write
    })
  state.selected.set(Selected::Note(id))
  store.refresh(&state)      ← re-reads lists from DB
```

Pattern is the same for every mutation:
1. View event → `main.rs` keyboard handler or component event handler.
2. `spawn(async move { store.mutation(...).await })`.
3. `DataStore` method calls `backend::current().<method>()`.
4. Backend impl executes (locally or over HTTP).
5. `store.refresh(&state)` re-fetches the affected list signals.

---

## UI internal structure

### State signals

Two Dioxus context objects are provided at the root and shared down the tree:

**`AppState`** (`state.rs`) — navigation state:
- `space_id: Signal<i64>` — active space
- `section: Signal<Section>` — active panel (AllNotes, Folder(id), AllPages, …)
- `selected: Signal<Selected>` — active item (Note(id), Page(id), File(id), None)
- `renaming: Signal<Option<Renaming>>` — in-progress rename operation
- `toast: Signal<Option<Toast>>` — transient notification with optional undo
- `space_dropdown_open: Signal<bool>`

**`DataStore`** (`store.rs`) — cached data signals (views only read these):
- `pages`, `notes`, `files`, `folders`, `spaces`, `trash_items`
- `note_counts`, `trash_count`
- `revision` — current DB revision number (from `db_revision` table)
- `schema_outdated` — true when on-disk DB is from a newer build
- `heatmap`, `timeline_selected_day`, `timeline_day_notes/pages`
- `current_note_urls`, `url_statuses` — link status indicators
- `open_note_id/updated_at`, `saves_in_flight` — collision detection
- `backend_online`, `pending_note_save` — offline write queue

### Polling loop

`RevisionIndicator` (in `main.rs`) runs a `use_future` loop that fires every 2 s:

```
store.poll(&state)
  1. Check db_schema_version → set schema_outdated if DB is newer than binary
  2. get_revision()
     OK  → backend came back online?
              yes → flush pending_note_save with 3-way merge
           revision changed?
              yes → refresh all data signals
     Err → set backend_online = false (offline banner appears)
  3. If open note: compare updated_at → push smartReplace into editor if changed
```

### View layout (three panels)

```
AppLayout
├── OfflineBanner          (document flow, above panels)
└── div.app-layout
    ├── div.app-keyboard-trap  (global onkeydown)
    │   ├── sidebar::Sidebar
    │   ├── div.list-panel-container
    │   │   └── list_pages | list_notes | list_files |
    │   │       list_search | list_trash | list_timeline | list_settings
    │   └── div.content-panel-container
    │       └── content_page | content_note | content_file |
    │           content_rules | content_spaces | content_empty
    ├── RevisionIndicator  (polling loop lives here)
    └── toast::Toast
```

Views read signals from `DataStore` and call mutation methods via `spawn`.
Views never call `backend::current()` directly.

### Note editor (Milkdown)

`content_note/` has its own sub-structure:
- `mod.rs` — orchestrator: mounts Milkdown via `document::eval`, listens for
  JS messages (`milkdownChange`, `milkdownAttach`, `milkdownUrlsFound`).
- `editor.rs` — Dioxus component wrapping the `<div id="milkdown-editor">`.
- `bridges.rs` — JS↔Rust message types.
- `attachments_panel.rs` — inline attachment upload/display.
- `actions.rs` — note action bar (move, trash, …).
- `folder_tree.rs` — folder picker popover.

Auto-save fires on every `milkdownChange` event. The `saves_in_flight` counter
prevents the poll loop from treating the resulting `updated_at` bump as an
external edit.

---

## lore-server HTTP API

All endpoints are `POST /api/<method>` with JSON bodies and responses, except
the two raw blob downloads (GET). The server runs on port `$LORE_PORT` (default
3000) and also serves the WASM bundle as static files under `/`.

Error envelope — every non-2xx response body:

```json
{ "code": "not_found" | "route_not_found" | "invalid_input" | "internal",
  "message": "..." }
```

### Full endpoint list

| Method | Endpoint |
|--------|----------|
| `get_revision` | `POST /api/get_revision` |
| `db_schema_version` | `POST /api/db_schema_version` |
| `list_spaces` | `POST /api/list_spaces` |
| `list_all_spaces` | `POST /api/list_all_spaces` |
| `get_active_space` | `POST /api/get_active_space` |
| `space_stats` | `POST /api/space_stats` |
| `touch_space` | `POST /api/touch_space` |
| `create_space` | `POST /api/create_space` |
| `rename_space` | `POST /api/rename_space` |
| `trash_space` | `POST /api/trash_space` |
| `restore_space` | `POST /api/restore_space` |
| `delete_space_permanent` | `POST /api/delete_space_permanent` |
| `list_folders` | `POST /api/list_folders` |
| `folder_note_counts` | `POST /api/folder_note_counts` |
| `create_folder` | `POST /api/create_folder` |
| `rename_folder` | `POST /api/rename_folder` |
| `delete_folder` | `POST /api/delete_folder` |
| `list_notes` | `POST /api/list_notes` |
| `list_note_ids_ordered` | `POST /api/list_note_ids_ordered` |
| `get_note` | `POST /api/get_note` |
| `create_note` | `POST /api/create_note` |
| `update_note` | `POST /api/update_note` |
| `move_note` | `POST /api/move_note` |
| `trash_note` | `POST /api/trash_note` |
| `restore_note` | `POST /api/restore_note` |
| `delete_note_permanent` | `POST /api/delete_note_permanent` |
| `find_notes_referencing_url` | `POST /api/find_notes_referencing_url` |
| `list_pages` | `POST /api/list_pages` |
| `list_page_ids_ordered` | `POST /api/list_page_ids_ordered` |
| `get_page` | `POST /api/get_page` |
| `archive_url` | `POST /api/archive_url` |
| `auto_archive_from_text` | `POST /api/auto_archive_from_text` |
| `check_urls_status` | `POST /api/check_urls_status` |
| `trash_page` | `POST /api/trash_page` |
| `restore_page` | `POST /api/restore_page` |
| `delete_page_permanent` | `POST /api/delete_page_permanent` |
| `update_page_status` | `POST /api/update_page_status` |
| `list_files` | `POST /api/list_files` |
| `get_file` | `POST /api/get_file` |
| `get_file_data` | `POST /api/get_file_data` |
| `insert_file` | `POST /api/insert_file` |
| `trash_file` | `POST /api/trash_file` |
| `restore_file` | `POST /api/restore_file` |
| `delete_file_permanent` | `POST /api/delete_file_permanent` |
| `list_attachments` | `POST /api/list_attachments` |
| `list_removed_attachments` | `POST /api/list_removed_attachments` |
| `get_attachment` | `POST /api/get_attachment` |
| `get_attachment_data` | `POST /api/get_attachment_data` |
| `insert_attachment` | `POST /api/insert_attachment` |
| `cleanup_orphaned_attachments` | `POST /api/cleanup_orphaned_attachments` |
| `restore_attachment` | `POST /api/restore_attachment` |
| `list_trash` | `POST /api/list_trash` |
| `trash_count` | `POST /api/trash_count` |
| `activity_by_day` | `POST /api/activity_by_day` |
| `activity_for_day` | `POST /api/activity_for_day` |
| `load_rules` | `POST /api/load_rules` |
| `search_pages_brief` | `POST /api/search_pages_brief` |
| `search_notes` | `POST /api/search_notes` |
| File download | `GET /api/files/{id}/raw` |
| Attachment download | `GET /api/attachments/{id}/raw` |

Binary data (`data`, screenshot bytes) travels as base64 strings in JSON
on the POST endpoints. The GET raw endpoints return the bytes directly with
`Content-Disposition: attachment`.

---

## lore-core public API

`lore-core` is the only shared crate. Its public surface has two tiers:

**Always available** (types only — compile in any target including WASM):

```
lore_core::db::
  NoteRow, NoteData
  WebPageRow, WebPageDetail, WebPageSnapshot, ArchiveOutcome, ClassificationRule
  FolderRow
  SpaceRow, SpaceStats
  FileRow, InsertFileOutcome
  AttachmentRow, InsertAttachmentOutcome
  TrashItem, TrashKind
  PageRef

lore_core::error::
  BackendError { code: ErrorCode, message: String }
  ErrorCode — NotFound | RouteNotFound | InvalidInput | Internal

lore_core::merge::
  three_way_merge(base, ours, theirs) → MergeResult

lore_core::url_extract::
  extract_urls(markdown: &str) → Vec<String>

lore_core::rules::
  classify(url, rules) → &str
  normalize_url(url) → String
  is_private_network(host) → bool
  is_tracking_param(key) → bool

lore_core::serde_b64   — serde helpers for Option<Vec<u8>> ↔ base64

lore_core::EXPECTED_DB_SCHEMA_VERSION: u32
```

**`sqlite` feature only** (requires rusqlite — not in WASM):

```
lore_core::db::
  open(path) → Result<Connection>          bootstrap: migrations + seed
  open_existing(path) → Result<Connection> runtime: no migration runner

  get_revision(&conn) → i64

  // Notes
  insert_note, get_note, list_notes, list_note_ids_ordered
  update_note, move_note_to_folder
  trash_note, restore_note, restore_note_safe, delete_note_permanent
  find_notes_referencing_url

  // Pages
  archive_url, auto_archive_from_text, check_urls_status
  ensure_page, find_page_by_url, insert_web_page, insert_snapshot
  list_pages, list_page_ids_ordered, get_page
  trash_page, restore_page, delete_page
  update_status, update_status_with_error
  load_rules

  // Folders
  insert_folder, list_folders, folder_note_counts
  rename_folder, delete_folder

  // Spaces
  insert_space, list_spaces, list_all_spaces, get_active_space
  touch_space, rename_space, space_stats
  trash_space, restore_space

  // Files
  insert_file, list_files, get_file, get_file_data
  trash_file, restore_file, delete_file_permanent, list_trashed_files

  // Attachments
  insert_attachment, list_attachments, list_removed_attachments
  get_attachment, get_attachment_data
  cleanup_orphaned_attachments, restore_attachment
  delete_attachments_for_note, list_attachment_ids_for_note

  // Trash / GC
  list_trash, trash_count, delete_space_permanent, cleanup_old_trash

  // Activity
  activity_by_day, activity_for_day

lore_core::search::
  search_web_pages(&conn, query, space_id, limit) → Vec<WebPageRow>
  search_web_pages_brief(...)                      → Vec<WebPageRow>
  search_notes(&conn, query, space_id, limit)      → Vec<NoteRow>
  list_pages_filtered(&conn, space_id, category, status, domain, limit)
  prepare_query(raw) → String   -- auto-appends * for short queries

lore_core::migrations::
  apply(&mut conn)              -- idempotent, transactional
  current_version(&conn) → u32
  EXPECTED_VERSION: u32
```

---

## Database

Single SQLite file, WAL mode, foreign keys on. Schema version tracked in
`PRAGMA user_version`. Current version: **7**.

### Tables (domain grouping)

| Table | Purpose |
|-------|---------|
| `space` | Workspaces. `last_used` drives active-space selection. |
| `note_folder` | Folders within a space. Self-referential `parent_id`. |
| `note` | Markdown notes. `title` + `body` stored as plain text. |
| `web_page` | Archived pages. `status`: `pending → archived / error`. |
| `web_page_snapshot` | HTML + screenshot blob for one archived page. |
| `file` | User-uploaded files (shared across spaces). |
| `attachment` | File attached to a specific note. |
| `classification_rule` | URL → category mapping. Seeded from `seed.sql`. |
| `db_revision` | Single row; incremented by triggers on every write. |

### Revision mechanism

Every INSERT/UPDATE/DELETE on content tables fires a trigger that increments
`db_revision.revision`. The polling loop reads this value every 2 s and
triggers a full refresh only when it changes — no timestamp polling, no
missed updates.

### Schema migrations

Migrations live in `crates/lore-core/migrations/NNNN_*.sql`, embedded via
`include_str!` in `migrations.rs`. Each migration runs in its own transaction
so a failed upgrade leaves the DB at a clean intermediate version. The DB is
refused on open if its version is higher than `EXPECTED_DB_SCHEMA_VERSION`
(prevents old binary from corrupting a newer schema).

Migration history:
- `0001` — initial schema
- `0002` — space soft-delete (`deleted_at`)
- `0003` — file table
- `0004` (code) — attachment size + SHA256 backfill
- `0005` — rewrite attachment markdown URLs
- `0006` (code) — unescape attachment link text
- `0007` — `db_revision` table + triggers

---

## Common paths and divergence points

| What | Shared path | Diverges at |
|------|-------------|-------------|
| All CRUD | `store.rs` method → `backend::current().<method>()` | `Backend` impl: `LocalBackend` vs `HttpBackend` |
| Data to DB | `lore_core::db::*` SQL | — (always the same code) |
| Types on the wire | `lore_core::db::*Row` structs | Desktop: direct Rust. Web: JSON serialize/deserialize |
| Binary blobs | `Vec<u8>` in Rust | Desktop: raw bytes. Web: base64 string in JSON body |
| Boot | `db::open` once, then `db::open_existing` per call | Desktop: in-process. Server: at startup + per handler |
| Error codes | `BackendError { code, message }` | Desktop: from `anyhow`. Web: parsed from HTTP body |
| Screenshots | `Option<Vec<u8>>` in `WebPageSnapshot` | Desktop: raw. Web: `serde_b64` → base64 JSON field |
| File downloads | Desktop: `rfd::AsyncFileDialog` | Web: `GET /api/files/{id}/raw` anchor |
