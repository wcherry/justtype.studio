-- Moderation, at the scale this product actually has: there is no moderator,
-- so a report goes to the only person with standing to act on it — the author
-- whose work the comment is attached to.
CREATE TABLE comment_reports (
    id          TEXT PRIMARY KEY NOT NULL,
    comment_id  TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    reporter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason      TEXT NOT NULL,
    detail      TEXT,
    created_at  TEXT NOT NULL,
    -- Reporting the same comment twice is the same report.
    UNIQUE (comment_id, reporter_id)
);
CREATE INDEX idx_comment_reports_comment ON comment_reports(comment_id);

-- An author's own list of people who do not get to read their work. Not a
-- platform ban: it is one writer's door.
CREATE TABLE blocked_readers (
    id         TEXT PRIMARY KEY NOT NULL,
    owner_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE (owner_id, user_id)
);
CREATE INDEX idx_blocked_readers_owner ON blocked_readers(owner_id);

-- Serial fiction is read as it is written, so one reader is always further in
-- than another. Withhold notes on scenes a reader has not reached: the whole
-- point of reading a serial is not knowing yet. Defaulted on when the work is
-- a serial, and the author can turn it off.
ALTER TABLE publications ADD COLUMN spoiler_safe INTEGER NOT NULL DEFAULT 0;
