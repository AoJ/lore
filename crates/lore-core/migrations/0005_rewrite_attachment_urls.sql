-- Rewrite legacy `lore://attachment/N` URLs to `https://attachment.lore.invalid/N`.
-- The custom `lore://` scheme wasn't recognized as a hyperlink by Milkdown's
-- markdown parser → links rendered as raw text. The HTTPS host name is bogus
-- (`.invalid` is reserved by RFC 2606 to never resolve) but lets Milkdown
-- render an <a> tag we can intercept via a click handler / markView.
-- REPLACE is idempotent — running on already-migrated bodies is a no-op.
UPDATE note SET
    body  = REPLACE(body,  'lore://attachment/', 'https://attachment.lore.invalid/'),
    title = REPLACE(title, 'lore://attachment/', 'https://attachment.lore.invalid/')
WHERE body LIKE '%lore://attachment/%' OR title LIKE '%lore://attachment/%';
