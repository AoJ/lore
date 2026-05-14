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
make check              # lint + check-arch + audit + tests (pre-PR)
make check-arch         # sentrux check (.sentrux/rules.toml)
make audit              # cargo-deny: licenses, advisories, duplicates
make mutants            # cargo-mutants on lore-core (slow, run on demand)
make verify             # cargo-kani proofs on lore-core (slow, run on demand)
make desktop            # run Dioxus desktop app
make serve              # run web server (axum)
make worker             # run archive worker
make migrate            # apply pending DB migrations
make db-version         # show DB version
make js-build           # rebuild crates/lore-ui/assets/milkdown.js
```

Database defaults to `./db.sqlite`; override with `DB=` or `LORE_DB=`.

### Dependency policy (`deny.toml`)

- Project itself is MIT (`LICENSE` at repo root, mirrored as `license = "MIT"`
  in every workspace `Cargo.toml`).
- License allow-list for dependencies (MIT, Apache-2.0, BSD-*, MPL-2.0, ISC,
  CC0, Unicode-3.0, Zlib, BSL-1.0, CDLA-Permissive-2.0). Anything else fails —
  keeps GPL/AGPL/LGPL out of the tree.
- RustSec advisories are checked; `ignore` lists `unmaintained` IDs from the
  Linux GTK3 stack (Dioxus transitive) and chromiumoxide. Revisit when Dioxus
  moves off GTK3.
- Duplicate versions and wildcard deps are `warn` only (wide Dioxus+axum+
  chromiumoxide tree → 29 duplicates currently, expected).

### Mutation testing (`.cargo/mutants.toml`)

`make mutants` runs `cargo mutants` against `lore-core` and reports any
mutation of the source that the test suite failed to catch. Last run: **0
missed** out of 234 viable mutants (37 unviable / equivalent), so every
boolean/comparison/counter/return-value mutation in `lore-core` is observable
from at least one test. Reruns are slow (≈30 min on M-series) and not in
`make check` — invoke when adding new pure logic in `lore-core`.

`version.rs` (env-injected version string + git SHA) is gated with
`#[cfg_attr(test, mutants::skip)]` because the values come from `env!` and any
"replace with empty string" mutant would need a brittle pin to the current
crate version / git SHA. The `mutants` crate is a dev-dep purely for that
attribute.

### Formal verification (`#[cfg(kani)] mod proofs`)

`make verify` runs Kani against pure parser/classifier functions in `lore-core`.
Harnesses live in `#[cfg(kani)] mod proofs` blocks next to the functions they
verify (`url_extract::extract_urls`, `search::prepare_query`,
`rules::is_private_network`, `rules::is_tracking_param`). They are invisible
to `cargo build` and `cargo test` — the `kani` crate is only injected by
`cargo kani`.

**Scope: fixed inputs, full-body symbolic execution.** Initial attempts to feed
symbolic `&str` values to these functions stalled CBMC on the internal
char-boundary loops in `core::str::trim`, `to_lowercase`, `find`, `parse`,
etc. (`run_utf8_validation`, `floor_char_boundary` ran tens of thousands of
iterations and exhausted memory). The harnesses therefore pass each function
a concrete input and let Kani symbolically execute every reachable branch
of the function body. What this catches that `cargo test` cannot:
panic-freedom, integer UB (overflow, division-by-zero), slice OOB, and
pointer-dereference soundness on every path reachable from the input — Kani
discharges 500 – 3 600 such checks per harness.

Branches covered: `is_private_network` × 6 (localhost / 127.0.0.1 /
192.168.x / 172.16-31 / 172.32 boundary / public host), `is_tracking_param`
× 4 (`utm_` prefix, mixed-case `utm_`, known dictionary key, plain key),
`prepare_query` × 2 (wildcard passthrough, empty input), `extract_urls`
× 3 (empty, bare URL, markdown link). One branch is **not** covered:
`prepare_query`'s `format!("{w}*")` no-operator path — `format!` blows
CBMC's bitvector budget even with a 4-byte constant input. Last run:
**15 / 15 successful**, total verification time ≈ 25 s. Required tools:
`cargo install --locked kani-verifier && cargo kani setup`. The `cfg(kani)`
lint is silenced via `check-cfg` in `lore-core/Cargo.toml`.

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
