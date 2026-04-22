## Project: lore

Personal knowledge management tool. Archive web pages, full-text search, notes (future).

### Architecture

```
crates/
├── lore-core/      # Pure library: DB, classification rules, search
│                     No network deps. Compiles to WASM and native.
├── lore-cli/       # CLI: lore add, lore search, lore list
├── lore-ui/        # Dioxus desktop app (also planned for web via same codebase)
├── lore-server/    # axum HTTP API + static web frontend (vanilla JS)
└── lore-worker/    # Standalone binary: fetches queued pages via headless Chrome
                      Communicates only through SQLite, no API.
```

### Key design decisions

- **Classification rules live in SQLite** (`classification_rule` table), not hardcoded. Seed data in `seed.sql`. No business logic in tag values or code branching on specific domains.
- **Tags** are removed. Were misused as business categories. Will be re-added when there's a real use case with free-form semantics.
- **Renderer (headless browser) is isolated** in `lore-worker`. Must run separately, ideally in a sandboxed environment (container, jail) with no internal network access. Auto-download of Chromium is temporary for testing only.
- **lore-core has zero network dependencies** — this allows Dioxus UI to compile for both desktop and web from one codebase.
- **Desktop app (Dioxus) calls lore-core directly** via Rust function calls, no HTTP, no ports, no firewall issues.
- **Web version** served by lore-server (axum), frontend is vanilla JS + Pico CSS calling JSON API.

### UI preferences

- **12px base font** for desktop density, not presentation
- **Light theme only** (`data-theme="light"`) — user has OS dark mode for system chrome but prefers light apps
- **List-based views**, not cards with large thumbnails
- **Pico CSS** as base, custom `app.css` for layout
- Apple Notes-like UX: fluid transitions, instant saves, no page jumping

### Search

- Currently SQLite FTS5 with auto-prefix (`tri` → `tri*`)
- FTS5 limitations: no substring/infix search (`*ili*` doesn't work), no typo tolerance
- Planned: replace with `milli` crate (Meilisearch core engine) as embedded library for prefix, typo tolerance, and language support. Keep FTS5 as fallback or remove.

### CLI usage

```
lore add <url> [url...]           # add URLs, classify via DB rules
lore add --batch <file>           # one URL per line, optionally URL<TAB>TITLE
lore search <query>               # FTS5 search (auto-prefix for short queries)
lore list [--category X] [--status X] [--domain X]
lore-worker --db <path> [url]     # archive queued pages or specific URL
```

### Build

```
make build          # build all crates
make test           # run tests
make desktop        # run Dioxus desktop app
make serve          # run web server (axum + static frontend)
make worker         # run archive worker
```

Database defaults to `./db.sqlite`, override with `DB=` or `LORE_DB=`.

### What's not done yet

- Dioxus web compilation (needs `wasm32-unknown-unknown` target + `dx` CLI)
- Notes module with hierarchical structure
- Trix editor integration for annotations
- Multi-context/scope switching
- Sync/replication
- milli search integration
- Remote renderer service API
- Readability extraction stored at archive time
