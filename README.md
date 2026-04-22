# lore

Personal knowledge management tool. Archive web pages, full-text search across saved content.

Built in Rust with SQLite (FTS5) storage. Uses headless Chrome for page rendering (with HTTP fallback).

## Build

```
cargo build --release
```

Binary: `target/release/lore` (single file, includes SQLite).

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

Tables:
- `web_page` -- URL, title, domain, category, status, timestamps
- `web_page_snapshot` -- versioned snapshots (HTML, plain text, screenshot)
- `web_page_fts` -- FTS5 full-text search index
- `classification_rule` -- URL classification rules (pattern, match_type, category, priority)

## Roadmap

- [ ] Remote renderer service (isolated headless Chrome behind API)
- [ ] Web UI
- [ ] Notes module with hierarchical structure
- [ ] Multi-context/scope switching
- [ ] Sync/replication
- [ ] Batch file input format for URL lists

## License

TBD
