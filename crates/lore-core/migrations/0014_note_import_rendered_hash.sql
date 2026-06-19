-- Second import hash for the attachment-aware importer (`import_md`).
--   import_hash          = hash of the RAW source file ("did the file change?")
--   import_rendered_hash = hash of the note body as stored after local links
--                          were rewritten to attachment URLs ("did I edit it in
--                          lore?"). NULL for notes imported before attachments
--                          (their body == raw, so import_hash is the fallback).
ALTER TABLE note ADD COLUMN import_rendered_hash TEXT;
