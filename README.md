# lore

Personal knowledge management tool. Archive web pages, full-text search across saved content.

Built in Rust with SQLite (FTS5) storage. Uses headless Chrome for page rendering (with HTTP fallback).

## Install (Homebrew, private tap)

Prebuilt releases are distributed through a private Homebrew tap
(`AoJ/homebrew-lore`). One `lore` **formula** ships everything: the CLI binaries
(`lore`, `lore-serve`, `lore-worker`) and, on macOS, the desktop app `Lore.app`.

> Why a formula and not a cask for the GUI: Homebrew is removing support for
> non-notarized casks (Gatekeeper-failing casks are disabled from 2026-09-01),
> and it dropped the `--no-quarantine` flag in 5.1. Formula downloads aren't
> quarantined, so the ad-hoc-signed `Lore.app` launches without the macOS
> "could not verify" block. (Once the app is notarized this can move back to a
> proper cask.)

One-time setup — the repo is private, so set a token with read access to
`AoJ/lore` in Homebrew's standard variable:

```bash
brew tap AoJ/lore
export HOMEBREW_GITHUB_API_TOKEN=github_pat_...   # read access to AoJ/lore
```

(If you also use `HOMEBREW_GITHUB_API_TOKEN` for another account, swap in the
lore-capable token when installing/upgrading lore.)

Then:

```bash
brew install lore   # CLI + (on macOS) Lore.app
brew upgrade        # new releases (run `brew update` first to refresh the tap)
```

On macOS, symlink the app into `/Applications` once so it's launchable and
pinnable to the Dock (the symlink survives `brew upgrade`):

```bash
ln -sfn "$(brew --prefix lore)/Lore.app" /Applications/Lore.app
open -a Lore
```

The database defaults to `~/Library/Application Support/lore/lore.db` (macOS).
Override with `LORE_DB` for the CLI / terminal-launched binaries. Note: an app
launched from the Dock/Finder does not inherit your shell environment, so
`LORE_DB` from a shell rc does not apply there — it uses the default location.

The tap formula is regenerated automatically on each tagged release (see
`tools/homebrew/lore.formula.rb` and the `homebrew` job in
`.github/workflows/release.yml`).

## Build

```
make build         # debug build all crates
make release       # release build all crates
make desktop       # run desktop app (debug)
make desktop-release
make serve         # run web server
make worker        # run archive worker
make test          # run all tests
make e2e           # end-to-end web UI tests (headless Chromium)
```

Binary: `target/release/lore` (single file, includes SQLite).

### JS editor bundle

The Dioxus desktop app embeds a Milkdown-based markdown editor. Source lives in `crates/lore-ui/js/` and is bundled by esbuild into `crates/lore-ui/assets/milkdown.js` (committed for repeatable Rust builds).

```
make js-install    # one-time: install npm deps in crates/lore-ui/js
make js-build      # rebuild crates/lore-ui/assets/milkdown.js
make js-watch      # rebuild on every save during JS development
make js-clean      # nuke node_modules and lockfile
```

Edit the editor at `crates/lore-ui/js/index.js` (plain JS — no TypeScript), run `make js-build`, then `make desktop-release`. To upgrade Milkdown bump versions in `crates/lore-ui/js/package.json` and rebuild.

### End-to-end tests

`make e2e` boots `lore-server` (random port, tmp DB) and drives the WASM frontend in headless Chromium via `chromiumoxide`. Each test spawns its own isolated server/browser pair, so tests run in parallel without shared state.

Coverage: sidebar boot, notes CRUD through the UI, cross-client refresh of an open note (`smartReplace` regression), attachment / file download via the `GET /api/{attachments,files}/{id}/raw` endpoints, structured error codes (`route_not_found` / `not_found` / `invalid_input`), and sidebar navigation.

Not part of `make check` — Chromium + a fresh WASM bundle are heavy prerequisites. Run explicitly when touching the web UI, the server API, or the editor bridge.

## Quick start

```bash
# Add URLs to archive queue
lore add https://example.com/article https://github.com/user/repo

# Add URLs from file (one URL per line, optionally URL<TAB>TITLE)
lore add --batch urls.txt

# Archive queued pages (fetches, renders, extracts text, indexes)
lore archive --limit 10

# Archive a specific URL
lore archive https://example.com/article

# Full-text search
lore search "freebsd zfs"
lore search '"exact phrase"'

# List pages with filters
lore list --category archive --status queued
lore list --domain github.com --limit 20
```

## Commands

### `lore add <url>...`

Add one or more URLs to the database. Each URL is classified against rules in the database and assigned a category.

- `--batch <file>` -- read URLs from file. One URL per line. Optionally `URL<TAB>TITLE`.

### `lore archive [url]`

Fetch and archive pages. Without arguments, processes queued pages from the database.

- `<url>` -- archive a specific URL
- `--limit N` -- max pages to process from queue (default: 10)

**Rendering:** Tries headless Chrome first (full JS rendering + screenshot). If Chrome is unavailable, falls back to plain HTTP fetch.

Set `LORE_BROWSER=/path/to/chromium` to specify the browser binary.

### `lore search <query>`

Full-text search across archived page content (SQLite FTS5).

- Supports: `"exact phrase"`, `word1 AND word2`, `word1 OR word2`, `word1 NOT word2`
- Czech diacritics normalized: "radio" finds "Radio"
- `--limit N` (default: 20)

### `lore list`

List pages from the database.

- `--category <archive|discard|local>`
- `--status <queued|fetching|archived|failed|skipped>`
- `--domain <partial_match>`
- `--limit N` (default: 50)

## Classification

URLs are classified by rules stored in `classification_rule` table in SQLite. Rules are evaluated by priority (highest first), first match wins. Default (no match) = `archive`.

Rule types:
- `domain` -- exact domain match (also matches `www.` prefix)
- `domain_suffix` -- matches domain and all subdomains
- `url_prefix` -- matches host+path prefix
- `url_contains` -- substring match anywhere in URL

Seed rules are loaded on first run (Google searches, login pages, SaaS dashboards, etc.). Edit rules directly in SQLite or via future web UI.

**Categories:**

| Category | Status after add | Meaning |
|----------|-----------------|---------|
| `archive` | `queued` | Content to fetch and preserve |
| `discard` | `skipped` | Noise (search results, login pages, SaaS dashboards) |
| `local` | `skipped` | Local/private network, unreachable |

Hard-coded rules (not in DB): `file://` -> local, `chrome://` -> discard, private IPs -> local.

## Rendering

The archiver supports two backends:

1. **Headless Chrome** (preferred) -- full JS rendering, screenshots, real page content
2. **HTTP fetch** (fallback) -- plain HTML download, text extraction via scraper

Chrome is tried first. If it fails (not installed, permissions, sandbox), all subsequent pages in the batch fall back to HTTP.

**For production:** The renderer should run as an isolated service (container, jail) without access to internal networks. The `Renderer` trait is designed for this -- swap `LocalRenderer` for a `RemoteRenderer` that calls an API.

## Database

SQLite at `~/.local/share/lore/lore.db` by default.

Override: `--db <path>` or `LORE_DB` env var.

For a current map of all tables and columns see
[`crates/lore-core/SCHEMA.md`](crates/lore-core/SCHEMA.md).

### Schema versioning and migrations

The DB stores its schema version in SQLite's `PRAGMA user_version`. Each
build of lore embeds an `EXPECTED_VERSION` in `crates/lore-core/src/migrations.rs`
and on every connection open:

- `db_version > expected` → app refuses to start (don't downgrade silently)
- `db_version < expected` → pending migrations are applied, each in its own
  transaction
- pre-versioning DB (`user_version = 0` but tables exist) → stamped to
  `expected` on first open

Migrations live in `crates/lore-core/migrations/NNNN_description.sql`,
embedded into the binary via `include_str!`. Migrations needing Rust code
(SHA256 backfill, regex rewrites) are registered as `Step::Code` entries in
the runner. **Linear forward-only**: never edit or reorder past migrations,
only append.

```
make db-version    # show current and expected versions for $DB
make migrate       # apply pending migrations (no UI)
```

Add a new migration:
1. Create `crates/lore-core/migrations/NNNN_what_changes.sql` (or write a
   Rust function for code migrations)
2. Append it to `MIGRATIONS` in `src/migrations.rs`
3. Bump `EXPECTED_VERSION`
4. Update `SCHEMA.md` to reflect the new state

## Roadmap

- [ ] Remote renderer service (isolated headless Chrome behind API)
- [ ] Web UI
- [ ] Notes module with hierarchical structure
- [ ] Multi-context/scope switching
- [ ] Sync/replication
- [ ] Batch file input format for URL lists

## License

TBD
