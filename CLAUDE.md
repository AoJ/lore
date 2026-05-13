## Project: lore

Personal knowledge management tool. Archive web pages, full-text search, notes, attachments — all organized by workspaces ("spaces") with hierarchical folders.

## Architecture

```
crates/
├── lore-core/      # Pure library: DB, classification, search, URL extraction.
│                     No network, no Dioxus. WASM-ready.
├── lore-cli/       # CLI: lore add | search | list | db-version | migrate
├── lore-ui/        # Dioxus desktop app
├── lore-server/    # axum HTTP API (web frontend, future)
└── lore-worker/    # Standalone: headless Chrome archiver, talks via SQLite only.
```

### Layering (enforced)

Only `lore-core` is shared. The four binaries (ui, server, worker, cli) must
NOT import each other — they communicate strictly through the SQLite DB.
Defined in `.sentrux/rules.toml`, checked by `make check-arch`.

```
ui      ┐
server  ├── core   (one-way; binaries are peers, never depend on each other)
worker  ┤
cli     ┘
```

## Module layout

**`lore-core/src/`**
- `db.rs` + `db/` — connection mgmt + submodule per domain (`web_page`, `note`,
  `folder`, `space`, `file`, `attachment`, `trash`, `activity`). Public API
  exported flat via `pub use`, so callers say `lore_core::db::list_notes`, not
  `lore_core::db::note::list_notes`.
- `rules.rs` — pure URL→category classifier (`classify`, `normalize_url`).
- `search.rs` — pure FTS5 query API (`search_web_pages`, `search_notes`,
  `list_pages_filtered`, `prepare_query`). No stdout — CLI does its own formatting.
- `url_extract.rs` — pure markdown-link / bare-URL extractor.
- `migrations.rs` — versioned schema migrations (`PRAGMA user_version` +
  `EXPECTED_VERSION` gate; refuses to start on a newer-than-known DB).

**`lore-ui/src/`**
- `main.rs` — boot. Calls `db::open` once, renders `StartupError` overlay if it
  fails (no more blank window on DB problems).
- `state.rs` — `AppState` signals (section, selected, toast, …).
- `store.rs` — `DataStore`: central signal cache + every mutation method.
  Single source of truth; views never touch DB directly.
- `data.rs` — UI helpers only: `db_path()`, `open_db()` (calls
  `lore_core::db::open_existing` — no migration runner per call), format helpers
  (size, ext, mime), `PageDetailView` adapter (NULL-title fallback, base64
  screenshot encoding), `open_in_browser`. No raw SQL, no business logic.
- `texts.rs` — every user-visible string.
- `keys.rs` — keyboard shortcuts.
- `views/` — flat panel components plus `content_note/` (orchestrator + editor,
  bridges, attachments_panel, actions, folder_tree).

### DB connection model

Two distinct entrypoints:

- `lore_core::db::open(path)` — bootstrap. Runs migrations, seeds default
  space + classification rules. Call **once** at startup (main, CLI command,
  worker, server).
- `lore_core::db::open_existing(path)` — runtime open. Just `Connection::open` +
  per-connection PRAGMAs. No migration runner, no seed count queries.
  Used by `data::open_db()` for every per-call connection in the UI.

Why both: UI polls every 2 s and calls open per mutation. Routing all of those
through `open()` would re-run migration checks ~60×/min for no reason and would
hide real bootstrap failures inside ad-hoc handlers. `StartupError` only triggers
when `open()` (the boot path) actually fails.

## Key design decisions

- **Classification rules live in SQLite** (`classification_rule` table). Seed in
  `seed.sql`. No domain-specific branching in code.
- **Tags removed** — were misused as categories. Re-add when there's a real
  free-form-semantics use case.
- **Renderer (headless Chrome) isolated** in `lore-worker`. Talks to UI only via
  SQLite. Should run sandboxed (container/jail) with no internal network.
- **`lore-core` has zero network deps** — lets Dioxus compile for both desktop
  and (future) WASM from one codebase.
- **Desktop calls core directly** via Rust function calls. No HTTP, no ports.
- **Web version** (when built): `lore-server` (axum) serves vanilla JS + Pico
  CSS frontend calling JSON API.
- **Boundaries enforced**, not just documented. `.sentrux/rules.toml` defines
  the layer graph; `sentrux check` (via `make check-arch`) fails if anyone
  introduces a forbidden import.

## UI preferences

- **12px base font** for desktop density, not presentation
- **Light theme only** (`data-theme="light"`) — user has OS dark mode for system
  chrome but prefers light apps
- **List-based views**, not cards with large thumbnails
- **Pico CSS** as base, custom `app.css` for layout
- Apple Notes-like UX: fluid transitions, instant saves, no page jumping

## Search

- SQLite FTS5 with auto-prefix (`tri` → `tri*`) for short queries
- FTS5 limits: no substring/infix (`*ili*` doesn't work), no typo tolerance
- Planned: replace with `milli` (Meilisearch core engine) for prefix, typo,
  and per-language tokenization. Keep FTS5 as fallback or remove.

## CLI usage

```
lore add <url> [url...]           # classify via DB rules + insert
lore add --batch <file>           # one URL per line, optionally URL<TAB>TITLE
lore search <query>               # FTS5 (auto-prefix for short queries)
lore list [--category X] [--status X] [--domain X]
lore db-version                   # show on-disk vs. expected schema version
lore migrate                      # run pending migrations without starting UI
lore-worker --db <path> [url]     # archive queued pages or one specific URL
```

## Build & dev

```
make build              # build all crates
make test               # run cargo test --workspace
make check              # lint + check-arch + tests (pre-PR)
make check-arch         # sentrux check (.sentrux/rules.toml)
make desktop            # run Dioxus desktop app
make serve              # run web server (axum)
make worker             # run archive worker
make migrate            # apply pending DB migrations
make db-version         # show DB version
make js-build           # rebuild crates/lore-ui/assets/milkdown.js
```

Database defaults to `./db.sqlite`; override with `DB=` or `LORE_DB=`.

## Organizational model

- **Space** — top-level isolation (e.g. "Personal", "Work"). Each space has its
  own folders, notes, pages. Switching space changes every panel.
  Files and classification rules are shared across spaces.
- **Folders** — hierarchical tree within a space. Contain notes, nest
  arbitrarily. Collapsible in sidebar.
- **Notes** — freeform markdown (Milkdown WYSIWYG). First line = title.
  Auto-saved on every keystroke. Can have attachments (images, files).
- **Web pages** — belong to a space, classified by rules, archived by worker.
- **Files** — shared across spaces, can be attached to notes.

See `UX-SPEC.md` for detailed view specifications and `PLAN.md` for the
implementation backlog.

## What's not done yet (high level)

- Dioxus web compilation (needs `wasm32-unknown-unknown` + `dx` CLI)
- `milli` search integration
- Sync / replication
- Browser extension / external API
- Tags (free-form, future)
- Remote renderer service (worker over HTTP, sandboxed)
- `tracing` crate + `RUST_LOG`; commit hash in error reports
- GitHub Actions CI workflow (Makefile gates exist; CI is a future add)

See `PLAN.md` for the granular task list.
