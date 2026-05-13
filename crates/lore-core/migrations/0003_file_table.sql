-- File library: documents/images uploaded into a space, dedup by hash,
-- soft-delete with 30-day cleanup.
CREATE TABLE file (
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

CREATE INDEX idx_file_space ON file(space_id, deleted_at);
CREATE INDEX idx_file_hash  ON file(hash);

CREATE TRIGGER trg_rev_file_i AFTER INSERT ON file BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER trg_rev_file_u AFTER UPDATE ON file BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER trg_rev_file_d AFTER DELETE ON file BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
