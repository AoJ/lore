-- Drop readability metadata fields that nobody actually uses. The Article
-- view doesn't show byline, excerpt, or reading-time anywhere — they were
-- speculative when m0011 added them. Trimming them now (before any UI
-- ever read them) keeps the snapshot row narrow and the worker code
-- focused on what's actually shipped.
--
-- `readability_html` and `readability_text` stay — those drive the
-- Article tab and the FTS index respectively.
--
-- Requires SQLite 3.35+ for ALTER TABLE … DROP COLUMN (rusqlite bundles
-- a much newer SQLite so this is fine).

ALTER TABLE web_page_snapshot DROP COLUMN byline;
ALTER TABLE web_page_snapshot DROP COLUMN excerpt;
ALTER TABLE web_page_snapshot DROP COLUMN reading_time_sec;
