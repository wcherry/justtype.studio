-- Site administration and moderation (Epic: staff roles, restrictions,
-- reports, the queue, the word filter, external moderation).
--
-- Nothing here touches private writing. Everything moderated is something
-- other people can already see.

-- Staff roles, and the account states staff can put an account in.
ALTER TABLE users ADD COLUMN is_moderator INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN deactivated_at TEXT;
-- Set by a closing restriction; cleared if it is lifted or overturned.
ALTER TABLE users ADD COLUMN closing_at TEXT;
ALTER TABLE users ADD COLUMN purge_at TEXT;

-- Who an audit line was about and why it was done. Plain columns on the
-- existing table: the moderation log is read from audit_events.
ALTER TABLE audit_events ADD COLUMN subject_id TEXT;
ALTER TABLE audit_events ADD COLUMN reason TEXT;
CREATE INDEX audit_events_by_subject ON audit_events (subject_id, created_at);

-- What a moderator removed or held is separate from the author's own delete,
-- so restoring never undoes an author's deletion.
ALTER TABLE comments ADD COLUMN moderation_state TEXT NOT NULL DEFAULT 'visible';
ALTER TABLE review_questions ADD COLUMN moderation_state TEXT NOT NULL DEFAULT 'visible';
ALTER TABLE review_answers ADD COLUMN moderation_state TEXT NOT NULL DEFAULT 'visible';
ALTER TABLE prompt_entries ADD COLUMN moderation_state TEXT NOT NULL DEFAULT 'visible';
ALTER TABLE prompt_critiques ADD COLUMN moderation_state TEXT NOT NULL DEFAULT 'visible';
-- The words as typed, kept for staff when the filter masked them.
ALTER TABLE comments ADD COLUMN original_body TEXT;
ALTER TABLE prompt_critiques ADD COLUMN original_body TEXT;

CREATE TABLE staff_notes (
    id         TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    author_id  TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX staff_notes_by_user ON staff_notes (user_id, created_at);

CREATE TABLE restrictions (
    id            TEXT PRIMARY KEY NOT NULL,
    user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL,   -- warning | ban | suspend | read_only
    scope         TEXT NOT NULL,   -- site | discussion
    starts_at     TEXT NOT NULL,
    ends_at       TEXT,            -- NULL: permanent
    reason_code   TEXT NOT NULL,   -- a section of the community guidelines
    reason_text   TEXT NOT NULL,   -- the message the writer sees
    internal_note TEXT,
    created_by    TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    lifted_at     TEXT,
    lifted_by     TEXT,
    lift_reason   TEXT,
    ended_notified_at TEXT,
    linked_items  TEXT           -- JSON array of queue item ids that prompted it
);
CREATE INDEX restrictions_by_user ON restrictions (user_id, created_at);

CREATE TABLE appeals (
    id             TEXT PRIMARY KEY NOT NULL,
    restriction_id TEXT NOT NULL UNIQUE REFERENCES restrictions(id) ON DELETE CASCADE,
    body           TEXT NOT NULL,
    state          TEXT NOT NULL,  -- open | upheld | overturned
    decided_by     TEXT,
    decision_text  TEXT,
    created_at     TEXT NOT NULL,
    decided_at     TEXT
);

-- Kept forever, and with no foreign keys: the row has to outlive the account.
CREATE TABLE permanent_bans (
    id              TEXT PRIMARY KEY NOT NULL,
    email           TEXT NOT NULL,
    email_canonical TEXT NOT NULL UNIQUE,
    former_user_id  TEXT,
    display_name    TEXT NOT NULL,
    reason_code     TEXT NOT NULL,
    reason_text     TEXT NOT NULL,
    restriction_id  TEXT,
    banned_by       TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    removed_at      TEXT,
    removed_by      TEXT,
    removed_reason  TEXT
);

CREATE TABLE blocked_domains (
    domain     TEXT PRIMARY KEY NOT NULL,
    note       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Reports on things that had no report table: Daily Prompts entries, reader
-- questions and answers, profiles and publications.
CREATE TABLE reports (
    id                TEXT PRIMARY KEY NOT NULL,
    subject_type      TEXT NOT NULL,
    subject_id        TEXT NOT NULL,
    subject_author_id TEXT NOT NULL,
    reporter_id       TEXT NOT NULL,
    reason            TEXT NOT NULL,
    detail            TEXT,
    created_at        TEXT NOT NULL,
    UNIQUE (subject_type, subject_id, reporter_id)
);
CREATE INDEX reports_by_author ON reports (subject_author_id, created_at);

-- One row per flagged thing, whatever flagged it.
CREATE TABLE moderation_items (
    id                TEXT PRIMARY KEY NOT NULL,
    subject_type      TEXT NOT NULL,   -- comment | question | answer | entry | critique | profile | publication | account
    subject_id        TEXT NOT NULL,
    subject_author_id TEXT NOT NULL,
    state             TEXT NOT NULL,   -- open | held | actioned | dismissed
    sources           TEXT NOT NULL,   -- JSON array of strings
    report_count      INTEGER NOT NULL,
    top_reason        TEXT,
    severity          INTEGER NOT NULL,
    labels            TEXT,            -- JSON, from the word filter / platform
    first_flagged_at  TEXT NOT NULL,
    last_flagged_at   TEXT NOT NULL,
    claimed_by        TEXT,
    claimed_until     TEXT,
    decided_by        TEXT,
    decided_at        TEXT,
    decision          TEXT,
    UNIQUE (subject_type, subject_id)
);
CREATE INDEX moderation_items_by_state ON moderation_items (state, severity, last_flagged_at);

CREATE TABLE word_filter_terms (
    id         TEXT PRIMARY KEY NOT NULL,
    term       TEXT NOT NULL,
    normalized TEXT NOT NULL,
    match_kind TEXT NOT NULL,   -- word | substring
    action     TEXT NOT NULL,   -- block | hold | flag | mask
    applies_to TEXT NOT NULL,   -- JSON array of surfaces
    note       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE word_filter_allow (
    word       TEXT PRIMARY KEY NOT NULL,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE moderation_settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_by TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE external_checks (
    id                 TEXT PRIMARY KEY NOT NULL,
    subject_type       TEXT NOT NULL,
    subject_id         TEXT NOT NULL,
    provider           TEXT NOT NULL,
    external_id        TEXT,
    state              TEXT NOT NULL,   -- pending | done | failed
    attempts           INTEGER NOT NULL,
    recommended_action TEXT,
    labels             TEXT,
    created_at         TEXT NOT NULL,
    checked_at         TEXT,
    next_attempt_at    TEXT NOT NULL
);
CREATE INDEX external_checks_due ON external_checks (state, next_attempt_at);

CREATE TABLE external_events (
    id          TEXT PRIMARY KEY NOT NULL,
    provider    TEXT NOT NULL,
    event_id    TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL,
    received_at TEXT NOT NULL,
    applied_at  TEXT,
    error       TEXT
);

-- Decisions we still owe the platform (closing its review item).
CREATE TABLE external_outbox (
    id          TEXT PRIMARY KEY NOT NULL,
    provider    TEXT NOT NULL,
    external_id TEXT NOT NULL,
    decision    TEXT NOT NULL,
    attempts    INTEGER NOT NULL,
    created_at  TEXT NOT NULL,
    sent_at     TEXT
);

-- A copy of someone's data, built when they can't sign in to ask for it (a
-- banned account, an account staff are deleting) and fetched by an emailed
-- link. No foreign keys: it has to survive the account.
CREATE TABLE emailed_exports (
    id            TEXT PRIMARY KEY NOT NULL,
    token_hash    TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL,
    path          TEXT NOT NULL,
    expires_at    TEXT NOT NULL,
    downloaded_at TEXT,
    created_at    TEXT NOT NULL
);
