-- Soft-delete support for spaces.
ALTER TABLE space ADD COLUMN deleted_at TEXT;
