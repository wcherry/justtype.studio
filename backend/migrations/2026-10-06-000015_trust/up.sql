-- What was done to an account or a work that matters afterwards: who
-- published, who was let in or shut out, what was deleted.
--
-- No foreign keys, on purpose. An audit trail that disappears with the thing
-- it records is not one: the line saying an account was deleted has to
-- outlive the account.
CREATE TABLE audit_events (
    id         TEXT PRIMARY KEY NOT NULL,
    actor_id   TEXT,
    work_id    TEXT,
    action     TEXT NOT NULL,
    detail     TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_audit_events_actor ON audit_events(actor_id, created_at);
CREATE INDEX idx_audit_events_work ON audit_events(work_id, created_at);

-- Which terms someone agreed to, and when. A version number rather than a
-- flag, so a change to the terms can ask again.
ALTER TABLE users ADD COLUMN terms_version INTEGER;
ALTER TABLE users ADD COLUMN terms_accepted_at TEXT;
