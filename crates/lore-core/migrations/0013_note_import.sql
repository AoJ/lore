-- Track markdown-folder imports so re-importing the same folder updates instead
-- of duplicating, and surfaces conflicts (a note edited in lore vs. the source
-- file changed). See `import_md.rs`.
--   import_source = path of the source file relative to the import root.
--   import_hash   = hash of the file content at the last successful import
--                   (the three-way merge "base").
ALTER TABLE note ADD COLUMN import_source TEXT;
ALTER TABLE note ADD COLUMN import_hash TEXT;

-- One imported note per (space, source path). Partial index so hand-created
-- notes (import_source IS NULL) stay unconstrained.
CREATE UNIQUE INDEX idx_note_import
    ON note(space_id, import_source) WHERE import_source IS NOT NULL;
