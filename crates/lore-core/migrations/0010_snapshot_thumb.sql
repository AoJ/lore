-- Thumbnail screenshot stored alongside the full-size PNG. The detail view
-- shows the thumbnail by default (cheap to ship); the full screenshot is
-- fetched lazily on click via a dedicated endpoint so we don't pay the
-- full-page PNG cost on every navigation.
--
-- Backfill is intentionally NOT done: legacy snapshots keep `screenshot_thumb
-- = NULL`. UI falls back to the full screenshot for those rows; re-archiving
-- the page (which produces a new snapshot via the worker) is what generates
-- a thumb. Avoids spending CPU on a one-off transcoding step that would
-- block startup and risk failure on weird old PNG payloads.

ALTER TABLE web_page_snapshot ADD COLUMN screenshot_thumb BLOB;
