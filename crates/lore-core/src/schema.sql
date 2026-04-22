PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS web_page (
    id              INTEGER PRIMARY KEY,
    url             TEXT NOT NULL UNIQUE,
    url_normalized  TEXT NOT NULL,
    title           TEXT,
    domain          TEXT NOT NULL,
    category        TEXT NOT NULL DEFAULT 'archive',
    status          TEXT NOT NULL DEFAULT 'queued'
                    CHECK(status IN ('queued','fetching','archived','failed','skipped')),
    source          TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_web_page_domain ON web_page(domain);
CREATE INDEX IF NOT EXISTS idx_web_page_category ON web_page(category);
CREATE INDEX IF NOT EXISTS idx_web_page_status ON web_page(status);
CREATE INDEX IF NOT EXISTS idx_web_page_url_normalized ON web_page(url_normalized);

CREATE TABLE IF NOT EXISTS web_page_snapshot (
    id          INTEGER PRIMARY KEY,
    web_page_id INTEGER NOT NULL REFERENCES web_page(id),
    version     INTEGER NOT NULL DEFAULT 1,
    html_content TEXT,
    plain_text  TEXT,
    screenshot  BLOB,
    fetched_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(web_page_id, version)
);

CREATE VIRTUAL TABLE IF NOT EXISTS web_page_fts USING fts5(
    title,
    plain_text,
    url,
    tokenize='unicode61 remove_diacritics 2',
    content=''
);

-- Classification rules: evaluated by priority (highest first), first match wins.
-- Default (no match) = archive.
CREATE TABLE IF NOT EXISTS classification_rule (
    id         INTEGER PRIMARY KEY,
    pattern    TEXT NOT NULL,
    match_type TEXT NOT NULL CHECK(match_type IN ('domain','domain_suffix','url_prefix','url_contains')),
    category   TEXT NOT NULL DEFAULT 'discard',
    priority   INTEGER NOT NULL DEFAULT 0,
    note       TEXT
);

CREATE INDEX IF NOT EXISTS idx_classification_rule_priority ON classification_rule(priority DESC);
