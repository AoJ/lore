-- Readability extraction columns. The worker runs dom_smoothie after the raw
-- HTML capture and stores the cleaned article alongside the original.
--
-- Layout:
--   readability_html  — cleaned `<article>` HTML (no nav, ads, sidebars).
--                       Rendered as the default Article view in the UI.
--   readability_text  — plain-text version of the same content. Indexed
--                       in FTS (preferred over raw plain_text where it
--                       exists — better signal-to-noise for search).
--   byline            — author string parsed from <meta>/<a rel="author">/etc.
--   excerpt           — short summary (first paragraph or <meta description>),
--                       shown under the title in the detail view.
--   reading_time_sec  — words / 200 wpm * 60. NULL when no readability extract.
--
-- All five columns default to NULL. NO BACKFILL — legacy snapshots stay
-- empty; UI falls back to plain_text. A re-extract CLI command can fill
-- these later on demand, but is intentionally not part of this migration:
-- pages without an extractable article (login walls, dashboards) would
-- keep returning NULL and a migration that "tries again every startup"
-- is the failure mode we want to avoid.

ALTER TABLE web_page_snapshot ADD COLUMN readability_html TEXT;
ALTER TABLE web_page_snapshot ADD COLUMN readability_text TEXT;
ALTER TABLE web_page_snapshot ADD COLUMN byline TEXT;
ALTER TABLE web_page_snapshot ADD COLUMN excerpt TEXT;
ALTER TABLE web_page_snapshot ADD COLUMN reading_time_sec INTEGER;
