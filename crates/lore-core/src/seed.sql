-- Seed classification rules.
-- These are common patterns for discarding noise. Edit via web UI or SQL.
-- Default (no rule match) = 'archive'.
-- match_type: domain (exact), domain_suffix (*.example.com), url_prefix, url_contains

-- Search engines results
INSERT OR IGNORE INTO classification_rule (pattern, match_type, category, priority, note)
VALUES
    ('www.google.com/search', 'url_prefix', 'discard', 100, 'Google search results'),
    ('google.com/search', 'url_prefix', 'discard', 100, 'Google search results'),

    -- Auth / login pages
    ('accounts.google.com', 'domain', 'discard', 90, 'Google auth'),
    ('/login', 'url_contains', 'discard', 50, 'Login pages'),
    ('/signin', 'url_contains', 'discard', 50, 'Signin pages'),
    ('/sign-in', 'url_contains', 'discard', 50, 'Sign-in pages'),
    ('/oauth', 'url_contains', 'discard', 50, 'OAuth pages'),
    ('/sso', 'url_contains', 'discard', 50, 'SSO pages'),

    -- Social feeds (not profile/article pages)
    ('www.linkedin.com/feed', 'url_prefix', 'discard', 80, 'LinkedIn feed'),

    -- Translators
    ('deepl.com', 'domain_suffix', 'discard', 80, 'DeepL translator'),

    -- SaaS dashboards (behind login, dynamic content)
    ('portal.azure.com', 'domain', 'discard', 70, 'Azure portal'),
    ('admin.google.com', 'domain', 'discard', 70, 'Google admin'),
    ('console.cloud.google.com', 'domain', 'discard', 70, 'GCP console'),
    ('console.online.net', 'domain', 'discard', 70, 'Online.net console'),

    -- File sharing (links expire)
    ('fastshare.cloud', 'domain', 'discard', 60, 'Fastshare - links expire'),

    -- Local / private (as category 'local')
    ('localhost', 'domain', 'local', 100, 'Localhost'),
    ('127.0.0.1', 'domain', 'local', 100, 'Loopback');
