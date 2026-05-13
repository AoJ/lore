-- Fill in missing db_revision triggers so every content table bumps the
-- counter on every change. Without these, the UI's polling loop misses:
--   - edits to classification rules in Settings (no triggers at all)
--   - updates/deletes on web_page_snapshot (only INSERT was triggered)

CREATE TRIGGER trg_rev_classification_rule_i AFTER INSERT ON classification_rule
    BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER trg_rev_classification_rule_u AFTER UPDATE ON classification_rule
    BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER trg_rev_classification_rule_d AFTER DELETE ON classification_rule
    BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;

CREATE TRIGGER trg_rev_snapshot_u AFTER UPDATE ON web_page_snapshot
    BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
CREATE TRIGGER trg_rev_snapshot_d AFTER DELETE ON web_page_snapshot
    BEGIN UPDATE db_revision SET revision = revision + 1 WHERE id = 1; END;
