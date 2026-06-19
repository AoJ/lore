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
- `import_md.rs` — idempotent markdown-folder → notes import. Identity =
  `<root_folder>/<path rel. to import root>` (stored on `note.import_source`);
  re-import is a three-way sync using two hashes (`import_hash` = raw file,
  `import_rendered_hash` = stored body) and aborts atomically on a note edited
  in lore. Local links → note attachments (link rewritten); `--prune` trashes
  notes whose source vanished. Used by `lore import`.
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
- **Web version**: `lore-server` (axum) exposes the `Backend` trait surface
  1:1 over `POST /api/<method>` JSON RPC. Per-request `db::open_existing`
  against the same SQLite file the desktop opens (W1 phase: shared DB on
  localhost for dev). The browser-side client is a Dioxus WASM bundle
  built via `dx` (`make web`) that mounts the same component tree as
  desktop; the only platform difference is which `Backend` impl gets
  registered (`LocalBackend` for desktop, `HttpBackend` for web).
  Binary blobs ride as base64 strings in JSON via `lore_core::serde_b64`;
  streaming octet-stream endpoints are a later optimization.
- **Build targets**: `lore-ui` has cargo features `desktop` (default,
  pulls rusqlite/rfd/dirs/tokio) and `web` (gloo-net/gloo-timers/
  wasm-bindgen-futures). `lore-core` mirrors this via its `sqlite`
  feature — WASM builds use `--no-default-features` to drop rusqlite +
  migrations + the SQL-touching functions; types and `BackendError`
  remain. `dx build --platform web` automatically passes
  `--no-default-features --features web`.
- **Smoke test (W3e)**: `make serve` (boots `lore-server` on port 3000,
  serves WASM bundle + API), then in the browser open
  `http://localhost:3000/`. Optionally `make desktop` in parallel —
  both clients see the same DB (per `LORE_DB` env or default
  `./db.sqlite`). Validate: notes list refreshes between clients;
  create-note on one shows on the other on the next poll tick (2 s);
  errors render with the structured `BackendError.code` (4xx/5xx body
  is JSON, frontend matches on code).
- **Error envelope**: every server response — handler error, route fallback,
  JSON-rejection — serializes `lore_core::error::BackendError` as
  `{ "code": "<snake_case>", "message": "..." }`. Codes are
  `route_not_found` / `not_found` / `invalid_input` / `internal`; the
  frontend branches on `code` (resource lookup miss vs. API drift vs.
  client bug vs. server issue), the `message` is for toasts/logs only.
  The `Backend` trait in `lore-ui` returns `Result<T, BackendError>` so
  desktop and web surface the same codes.
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
lore import <dir> --space <name>  # idempotent markdown-folder import (notes)
       [--folder <name>] [--prune] [--dry-run]
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
make web                # build WASM bundle via dx, stage in lore-server/static/
make e2e                # integration tests: spawn lore-serve + drive WASM frontend
                        # in headless Chromium (chromiumoxide). Slow, on-demand.
```

Database defaults to `./db.sqlite`; override with `DB=` or `LORE_DB=`.

### Nix dev environment (`dev-env/`)

Pinned toolchain for NixOS/Linux (and macOS) lives in `dev-env/flake.nix`
(+ `flake.lock`). Two equivalent entrypoints:

```
nix develop ./dev-env             # interactive dev shell (default, lean)
nix build ./dev-env#wrapper       # → dev-env/result/bin/wrapper
                                  #   (rebuild to dev-env/result, not ./result:
                                  #    nix build ./dev-env#wrapper --out-link dev-env/result)
dev-env/result/bin/wrapper make check   # run any command inside the env
```

Two shells: `default` (day-to-day dev + nix CI jobs) and `cross` (`nix
develop ./dev-env#cross`, adds the cross gcc toolchains + per-target linker
env). The cross toolchains build mingw/gnu gcc from source (no binary cache
on this host), so they live in the `cross` shell only — entering `default`
stays fast. `make cross*` wraps itself into the `cross` shell automatically.

The shell provides: Rust stable + `wasm32-unknown-unknown` (rust-overlay),
`dx` (dioxus-cli), `wasm-bindgen-cli` **pinned to the `wasm-bindgen` version
in `Cargo.lock`** (`dx` requires an exact match and can't run its
self-downloaded binaries on NixOS — when `cargo update` bumps the crate, run
`make update-deps` (or `make update-wasm-bindgen`) to resync the flake's
version + both fixed-output hashes automatically; never hand-edit them),
binaryen, cargo-deny, cargo-mutants,
node (milkdown bundle), CloakBrowser (a fingerprint-patched Chromium with
CDP; from the `cloakbrowser` flake input, Linux-only, exported as
`LORE_BROWSER` for worker/e2e), and the GTK3/WebKitGTK stack for the
desktop build.
Not in nix: `sentrux` (not packaged) and Kani (needs `cargo kani setup`).

**Cross-compilation (headless crates only).** The Linux dev shell carries
the `x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnu` rust targets plus
their cross C toolchains (`pkgsCross.gnu64` / `pkgsCross.mingwW64`), wired to
cargo via `CARGO_TARGET_*_LINKER` + `CC_*`/`CXX_*` env. cc-rs deps (bundled
SQLite, ring) cross cleanly; the worker uses `rustls`, so no OpenSSL.

```
cargo build --release --target x86_64-unknown-linux-gnu -p lore-cli -p lore-server -p lore-worker
cargo build --release --target x86_64-pc-windows-gnu   -p lore-cli -p lore-server
```

Name the crates — don't pass `--workspace`: `lore-ui` (wry → WebKitGTK on
Linux / WebView2 on Windows) and `lore-e2e` don't cross-build. Scope by
design: **desktop `lore-ui` is macOS-only and built natively** (its `wry`
GUI + `openssl-sys` via the devtools websocket make cross from aarch64
impractical); Windows/x86-Linux users get the **WASM web UI** (`make web` +
`lore-server`). `lore-worker` stays Linux-only (drives CloakBrowser), so it
has no Windows target.

Requires `experimental-features = nix-command flakes` in nix.conf.

**CI** (`.github/workflows/`): `ci.yml` runs `fmt` (auto-commit) on rustup
plus `checks` (clippy/test/audit) and `web` inside the nix dev shell;
`auto-tag.yml` (on push to main) reads the `[workspace.package]` version and,
if no `v<version>` tag exists yet, creates+pushes it (via `DEPS_UPDATE_TOKEN`
so the tag triggers release.yml) — so a release is cut simply by bumping the
version in a merged PR. `release.yml` (on `v*` tags) builds the web bundle via
nix and the OS binaries natively (linux x86/arm + windows `.exe` + macOS
desktop + macOS `Lore.app`), then bumps the `AoJ/homebrew-lore` tap.
`deps-update.yml` (weekly cron) runs `cargo update` + `nix flake update` +
the wasm-bindgen resync and opens a PR that ci.yml tests, auto-merging when
green. **The PR must be opened by a real PAT** (`DEPS_UPDATE_TOKEN` secret) —
PRs from the default `GITHUB_TOKEN` don't trigger ci.yml — and auto-merge
needs "Allow auto-merge" + a branch-protection required-checks rule on main.

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
mutation of the source that the test suite failed to catch. Last full run
(2026-06-11, this host, single core): **499 mutants, 71 initially missed**.
Those broke down as 56 real coverage gaps — since fixed — and 15 false
positives in the `#[cfg(kani)] mod proofs` harnesses (never compiled under
`cargo test`, so unkillable here; now excluded via `exclude_re`, along with two
behaviourally-equivalent mutants in `merge::lcs_pairs` — the `||→&&` fast-path
guard and the `dp[i][j+1]→dp[i][j]` traceback read, both with proofs in
`mutants.toml`). The 56 real gaps were concentrated in pure / under-asserted
code the existing suite skirted: `merge.rs`'s diff/LCS/3-way core (the
identity-law proptests short-circuit on the `ours==base` / `theirs==base` early
returns, and the old unit tests asserted `.contains()` rather than exact text),
several `db/web_page.rs` accessors that were only checked for "doesn't error"
(`list_page_versions`, `get_snapshot_full_screenshot`, `delete_page_version`,
`request_reachive`, version increment, `compute_change_summary` arithmetic),
`export.rs` (`compact_stamp` / `slug_safe` edge chars), and `migrations.rs`
(`m0009` cleanup effect). All now covered — `merge.rs`'s LCS dp recurrence by an
exhaustive maximal-common-subsequence oracle and its 3-way apply by multi-hunk
exact-output cases; the rest by exact-value / effect-asserting tests. A scoped
re-run over the four touched files (2026-06-11) is **0 missed** — 227 caught,
8 timeouts (the `*=` infinite-loop mutants, detected by hang), 11 unviable.
Reruns are slow (≈2.5–3.5 h on this single-core host; the infinite-loop mutants
are killed by timeout, which dominates) and not in `make check` — invoke when
adding new pure logic in `lore-core`.

`version.rs` (env-injected version string + git SHA) is gated with
`#[cfg_attr(test, mutants::skip)]` because the values come from `env!` and any
"replace with empty string" mutant would need a brittle pin to the current
crate version / git SHA. The `mutants` crate is a dev-dep purely for that
attribute.

### E2E integration tests (`crates/lore-e2e/`)

`make e2e` spawns the real `lore-serve` binary as a subprocess (random
port, tmp DB) and drives the WASM frontend through headless Chromium via
`chromiumoxide`. Each `TestApp::spawn()` is fully isolated — tests run in
parallel, no shared state. Drop kills the server and removes the temp DB.

`make e2e` depends on `make web` (bundle staged) + `cargo build -p
lore-server` (binary). It's NOT part of `make check` (which excludes
`lore-e2e` from the workspace test invocation) because Chromium + a fresh
bundle are heavy prerequisites — keep the pre-PR gate fast.

Helpers in `crates/lore-e2e/src/lib.rs` cover the common moves:
`wait_for(selector, timeout)`, `click(selector)`, `text(selector)`,
`screenshot(path)`, `api_post(method, body)` (direct seed via
`fetch` from inside the page so cookies/headers match), and
`wait_until(predicate, timeout)` for polling-driven assertions
(revision bumps, list-item count changes).

Test layout: `tests/smoke.rs` (boot/render), `tests/notes.rs`
(CRUD through UI). Grow with one test file per feature area.

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

**NixOS caveat:** `cargo kani setup` does not work out of the box on this
NixOS host. It drives `rustup` to install a pinned nightly
(`rustup toolchain install nightly-…` → "No such file or directory" — no
rustup; the toolchain comes from the nix rust-overlay), and the release bundle
it downloads is FHS-linked (won't run without an FHS wrapper / `steam-run`).
Kani is **not** packaged in the pinned nixpkgs either (a `pkgs.cargo-kani`
attempt breaks `nix develop` — it doesn't exist), so `make verify` is currently
deployment-gated on this machine. The proof harnesses are unchanged, so the
last-known **15 / 15** still stands; re-run once Kani is wired into the
deployment (rustup-in-FHS, or a packaged kani derivation).

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
