# Database schema

> **Authority**: the SQL files in `crates/lore-core/migrations/` plus the Rust
> code migrations in `src/migrations.rs` are the source of truth. This
> document is a human-readable companion — when it disagrees with the
> migrations, the migrations are right.

The DB stores its schema version in SQLite's built-in `PRAGMA user_version`.
The application embeds an `EXPECTED_VERSION` constant; on connect it applies
any pending migrations or refuses to start if the DB is from a newer build.
See `src/migrations.rs` for the runner.

CLI helpers:

```
lore --db <path> db-version    # print current + expected schema version
lore --db <path> migrate       # apply pending migrations (no UI)
make db-version                # same, on $DB
make migrate
```

---

## Tables

### `space`
Top-level organizational unit. Every note, folder, web page belongs to one.
Files are also scoped per-space.

| column      | type    | notes                                           |
|-------------|---------|-------------------------------------------------|
| id          | INTEGER | primary key                                     |
| name        | TEXT    | display name                                    |
| color       | TEXT    | optional accent color                           |
| last_used   | TEXT    | ISO timestamp; "active space" = max(last_used)  |
| deleted_at  | TEXT    | soft-delete timestamp; non-NULL = trashed       |
| created_at  | TEXT    | ISO timestamp                                   |

### `web_page`
A URL queued for archival or already archived. Worker fetches asynchronously.

| column          | type    | notes                                           |
|-----------------|---------|-------------------------------------------------|
| id              | INTEGER | primary key                                     |
| url             | TEXT    | UNIQUE; original URL as entered                 |
| url_normalized  | TEXT    | tracking params stripped (see `rules.rs`)       |
| title           | TEXT    | extracted by worker, nullable                   |
| domain          | TEXT    | host name                                       |
| category        | TEXT    | from classification rules; default `archive`    |
| status          | TEXT    | `queued` / `fetching` / `archived` / `failed` / `skipped` |
| source          | TEXT    | how the URL got here (e.g. `note`, `cli`)       |
| space_id        | INTEGER | → space.id                                      |
| last_error      | TEXT    | nullable; latest worker error                   |
| trashed_at      | TEXT    | soft-delete timestamp                           |
| created_at      | TEXT    |                                                 |
| updated_at      | TEXT    |                                                 |

Indexes on: `domain`, `category`, `status`, `url_normalized`, `(space_id, trashed_at)`.

### `web_page_snapshot`
Per-version archived content for a `web_page`. One row per fetch.

| column        | type    | notes                                           |
|---------------|---------|-------------------------------------------------|
| id            | INTEGER | primary key                                     |
| web_page_id   | INTEGER | → web_page.id                                   |
| version       | INTEGER | starts at 1; increment on re-archive            |
| html_content  | TEXT    | full HTML                                       |
| plain_text    | TEXT    | extracted plain text (used by FTS)              |
| screenshot    | BLOB    | PNG, nullable (HTTP fallback has no screenshot) |
| fetched_at    | TEXT    |                                                 |

Unique on `(web_page_id, version)`.

### `web_page_fts`
FTS5 virtual table indexing snapshots' `title`, `plain_text`, `url`.
Tokenizer: `unicode61 remove_diacritics 2`. `content=''` means external content
mode — we manage indexing manually.

### `note_folder`
Hierarchical folder tree, scoped to a space.

| column       | type    | notes                                           |
|--------------|---------|-------------------------------------------------|
| id           | INTEGER | primary key                                     |
| name         | TEXT    | display name                                    |
| parent_id    | INTEGER | → note_folder.id; NULL = root                   |
| space_id     | INTEGER | → space.id                                      |
| sort_order   | INTEGER | reserved; not yet used in UI                    |
| created_at   | TEXT    |                                                 |

### `note`
A free-text note. First line of `title + "\n" + body` shown as title in UI.
`body` is markdown (Milkdown serialization).

| column       | type    | notes                                           |
|--------------|---------|-------------------------------------------------|
| id           | INTEGER | primary key                                     |
| title        | TEXT    | first line                                      |
| body         | TEXT    | markdown                                        |
| folder_id    | INTEGER | → note_folder.id; NULL = root of space          |
| space_id     | INTEGER | → space.id                                      |
| created_at   | TEXT    |                                                 |
| updated_at   | TEXT    |                                                 |
| deleted_at   | TEXT    | soft-delete                                     |

### `note_fts`
FTS5 virtual table over `(title, body)`. External content.

### `note_attachment`
Files attached to a specific note (per-note dedup by `name + hash`). The note
body references each attachment as a markdown link with URL
`https://attachment.lore.invalid/<id>`.

| column       | type    | notes                                           |
|--------------|---------|-------------------------------------------------|
| id           | INTEGER | primary key                                     |
| note_id      | INTEGER | → note.id                                       |
| name         | TEXT    | original filename                               |
| mime_type    | TEXT    | nullable                                        |
| size         | INTEGER | bytes                                           |
| hash         | TEXT    | SHA256 hex                                      |
| data         | BLOB    | bytes                                           |
| created_at   | TEXT    |                                                 |
| deleted_at   | TEXT    | soft-delete; hard-deleted after 30 days         |

### `file`
Space-scoped file library. Independent from `note_attachment` (a note attachment
with the same content stays separate from a `file` row).

| column       | type    | notes                                           |
|--------------|---------|-------------------------------------------------|
| id           | INTEGER | primary key                                     |
| name         | TEXT    | filename                                        |
| mime_type    | TEXT    | nullable                                        |
| size         | INTEGER | bytes                                           |
| hash         | TEXT    | SHA256 hex; dedup key per space                 |
| data         | BLOB    | bytes                                           |
| space_id     | INTEGER | → space.id                                      |
| created_at   | TEXT    |                                                 |
| deleted_at   | TEXT    | soft-delete                                     |

### `classification_rule`
Rules used to classify URLs into categories on insert. Seeded from
`src/seed.sql`; user-editable in the Settings panel.

| column      | type    | notes                                                       |
|-------------|---------|-------------------------------------------------------------|
| id          | INTEGER | primary key                                                 |
| pattern     | TEXT    | what to match                                               |
| match_type  | TEXT    | `domain` / `domain_suffix` / `url_prefix` / `url_contains`  |
| category    | TEXT    | output category (`archive`, `discard`, …)                   |
| priority    | INTEGER | higher wins                                                 |
| note        | TEXT    | freeform description                                        |

### `db_revision`
Single-row counter (id always 1) bumped by `AFTER INSERT/UPDATE/DELETE`
triggers on every content table. The UI polls this once and re-renders only
when the counter changes — no per-table polling.

## Triggers

For every content table (`web_page`, `web_page_snapshot`, `note`, `note_folder`,
`space`, `note_attachment`, `file`) there is a trigger that increments
`db_revision.revision` after every change.

## Out-of-DB state

- `journal_mode = WAL` and `foreign_keys = ON` are PRAGMAs set per-connection
  in `db::open()`, **not** in migrations (SQLite forbids journal_mode change
  inside a transaction).
- `PRAGMA user_version` holds the migration version. Read it with
  `lore db-version` or `sqlite3 db.sqlite "PRAGMA user_version"`.
