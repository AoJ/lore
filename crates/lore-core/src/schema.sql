PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- Spaces (top-level organizational unit)
CREATE TABLE IF NOT EXISTS space (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    color      TEXT,
    last_used  TEXT,
    deleted_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

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
    space_id        INTEGER REFERENCES space(id),
    last_error      TEXT,
    trashed_at      TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_web_page_domain ON web_page(domain);
CREATE INDEX IF NOT EXISTS idx_web_page_category ON web_page(category);
CREATE INDEX IF NOT EXISTS idx_web_page_status ON web_page(status);
CREATE INDEX IF NOT EXISTS idx_web_page_url_normalized ON web_page(url_normalized);
CREATE INDEX IF NOT EXISTS idx_web_page_space ON web_page(space_id, trashed_at);

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

-- Note folders (hierarchical)
CREATE TABLE IF NOT EXISTS note_folder (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    parent_id  INTEGER REFERENCES note_folder(id),
    space_id   INTEGER REFERENCES space(id),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_note_folder_space ON note_folder(space_id);

-- Notes
CREATE TABLE IF NOT EXISTS note (
    id         INTEGER PRIMARY KEY,
    title      TEXT NOT NULL DEFAULT '',
    body       TEXT NOT NULL DEFAULT '',
    folder_id  INTEGER REFERENCES note_folder(id),
    space_id   INTEGER REFERENCES space(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_note_space ON note(space_id, deleted_at);

-- Note full-text search
CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(
    title,
    body,
    tokenize='unicode61 remove_diacritics 2',
    content=''
);

-- Note attachments (images, files inline-blocks)
CREATE TABLE IF NOT EXISTS note_attachment (
    id         INTEGER PRIMARY KEY,
    note_id    INTEGER NOT NULL REFERENCES note(id),
    name       TEXT NOT NULL,
    mime_type  TEXT,
    size       INTEGER NOT NULL DEFAULT 0,
    hash       TEXT NOT NULL DEFAULT '',
    data       BLOB NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_note_attachment_note ON note_attachment(note_id);
-- idx_note_attachment_deleted is created in db.rs after the size/hash/deleted_at migration,
-- so it's safe both for fresh installs (column exists) and old DBs (migration adds column first).

-- Trigger: revision bump on attachment changes
CREATE TRIGGER IF NOT EXISTS trg_rev_attachment_i AFTER INSERT ON note_attachment BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_attachment_u AFTER UPDATE ON note_attachment BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_attachment_d AFTER DELETE ON note_attachment BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;

-- Classification rules
CREATE TABLE IF NOT EXISTS classification_rule (
    id         INTEGER PRIMARY KEY,
    pattern    TEXT NOT NULL,
    match_type TEXT NOT NULL CHECK(match_type IN ('domain','domain_suffix','url_prefix','url_contains')),
    category   TEXT NOT NULL DEFAULT 'discard',
    priority   INTEGER NOT NULL DEFAULT 0,
    note       TEXT
);

CREATE INDEX IF NOT EXISTS idx_classification_rule_priority ON classification_rule(priority DESC);

-- Global revision counter — incremented on every data change
CREATE TABLE IF NOT EXISTS db_revision (
    id       INTEGER PRIMARY KEY CHECK(id = 1),
    revision INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO db_revision (id, revision) VALUES (1, 0);

-- Triggers to auto-increment revision on any data change
CREATE TRIGGER IF NOT EXISTS trg_rev_web_page_i AFTER INSERT ON web_page BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_web_page_u AFTER UPDATE ON web_page BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_web_page_d AFTER DELETE ON web_page BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_note_i AFTER INSERT ON note BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_note_u AFTER UPDATE ON note BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_note_d AFTER DELETE ON note BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_folder_i AFTER INSERT ON note_folder BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_folder_u AFTER UPDATE ON note_folder BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_folder_d AFTER DELETE ON note_folder BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_space_i AFTER INSERT ON space BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_space_u AFTER UPDATE ON space BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_space_d AFTER DELETE ON space BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_snapshot_i AFTER INSERT ON web_page_snapshot BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;

-- Files (scoped to space, shared library of uploaded documents/images)
CREATE TABLE IF NOT EXISTS file (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    mime_type  TEXT,
    size       INTEGER NOT NULL DEFAULT 0,
    hash       TEXT NOT NULL DEFAULT '',
    data       BLOB NOT NULL,
    space_id   INTEGER REFERENCES space(id),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_file_space ON file(space_id, deleted_at);
CREATE INDEX IF NOT EXISTS idx_file_hash  ON file(hash);

CREATE TRIGGER IF NOT EXISTS trg_rev_file_i AFTER INSERT ON file BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_file_u AFTER UPDATE ON file BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER IF NOT EXISTS trg_rev_file_d AFTER DELETE ON file BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
