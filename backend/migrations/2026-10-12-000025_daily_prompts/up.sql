-- Daily Prompts: a prompt a day, written, shared and critiqued; the job runner
-- that keeps the days turning; site administrators; and an inbox.

-- The first role that belongs to the whole site rather than to one work.
-- Set from the command line only, never through the API.
ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0;
-- When turning text into a project, whether a blank paragraph (double Enter)
-- separates scenes as well as a `--` line does.
ALTER TABLE users ADD COLUMN double_enter_splits_scenes INTEGER NOT NULL DEFAULT 0;

-- One row per run of a scheduled job, keyed by what the run is for (the day,
-- for a daily job). The key's uniqueness is the guard: a restart, a missed
-- tick or a second process cannot run the same thing twice.
CREATE TABLE job_runs (
    job         TEXT NOT NULL,
    key         TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    error       TEXT,
    attempts    INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (job, key)
);

-- Mail written inside the transaction that decided to send it, and sent
-- afterwards by the runner, so a send can never be half-recorded.
CREATE TABLE mail_queue (
    id              TEXT PRIMARY KEY NOT NULL,
    to_address      TEXT NOT NULL,
    subject         TEXT NOT NULL,
    body            TEXT NOT NULL,
    html            TEXT,
    -- JSON: [[name, value], ...]
    headers         TEXT,
    created_at      TEXT NOT NULL,
    attempts        INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TEXT NOT NULL,
    -- After this, a failing message is given up rather than sent late.
    give_up_at      TEXT NOT NULL,
    sent_at         TEXT,
    failed_at       TEXT,
    last_error      TEXT
);
CREATE INDEX idx_mail_queue_due ON mail_queue(sent_at, failed_at, next_attempt_at);

-- A row with no day is in the queue, ordered by position. A row with a day is
-- that day's prompt: pinned by an admin, or taken from the queue the day
-- before. It opens when its (Pacific) day starts.
CREATE TABLE daily_prompts (
    id         TEXT PRIMARY KEY NOT NULL,
    day        TEXT UNIQUE,
    position   INTEGER,
    body       TEXT NOT NULL,
    -- `curated`, `ai`, or `evergreen` (the built-in list). Never shown to writers.
    source     TEXT NOT NULL DEFAULT 'curated',
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_daily_prompts_queue ON daily_prompts(position) WHERE day IS NULL;

-- Taking part. Leaving keeps the row (and the writer's preferences) with
-- `left_at` set; rejoining clears it.
CREATE TABLE prompt_enrollments (
    user_id           TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    enrolled_at       TEXT NOT NULL,
    left_at           TEXT,
    email_enabled     INTEGER NOT NULL DEFAULT 0,
    -- `HH:MM` in `email_timezone` (IANA).
    email_local_time  TEXT NOT NULL DEFAULT '07:00',
    email_timezone    TEXT NOT NULL DEFAULT 'America/Los_Angeles',
    unsubscribe_token TEXT NOT NULL UNIQUE,
    updated_at        TEXT NOT NULL
);

-- One answer per writer per prompt. The body is a ProseMirror document, the
-- same shape a scene's is, kept in the row rather than a file: entries are
-- short, and deleting one has to take its critiques in the same transaction.
CREATE TABLE prompt_entries (
    id                TEXT PRIMARY KEY NOT NULL,
    prompt_id         TEXT NOT NULL REFERENCES daily_prompts(id),
    author_id         TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body              TEXT NOT NULL,
    content_hash      TEXT NOT NULL,
    word_count        INTEGER NOT NULL DEFAULT 0,
    -- `draft`, `submitted` or `withdrawn`.
    state             TEXT NOT NULL DEFAULT 'draft',
    -- Set by the first submission and never again.
    submitted_at      TEXT,
    edited_at         TEXT,
    -- The last work made from it, for "Turned into …".
    converted_work_id TEXT REFERENCES works(id) ON DELETE SET NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    UNIQUE (prompt_id, author_id)
);
CREATE INDEX idx_prompt_entries_feed ON prompt_entries(prompt_id, state, submitted_at);
CREATE INDEX idx_prompt_entries_author ON prompt_entries(author_id);

-- A reply is a critique on a critique, to any depth. Separate from `comments`,
-- which belong to publications and versions. Deleting one that has replies
-- keeps it as a placeholder (`deleted_at`), as comments do.
CREATE TABLE prompt_critiques (
    id         TEXT PRIMARY KEY NOT NULL,
    entry_id   TEXT NOT NULL REFERENCES prompt_entries(id) ON DELETE CASCADE,
    parent_id  TEXT REFERENCES prompt_critiques(id) ON DELETE CASCADE,
    -- Not cascaded: a departing account's critiques are blanked and credited
    -- to the former member, so the replies under them still read.
    author_id  TEXT NOT NULL REFERENCES users(id),
    body       TEXT NOT NULL,
    edited_at  TEXT,
    deleted_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_prompt_critiques_entry ON prompt_critiques(entry_id, created_at);
CREATE INDEX idx_prompt_critiques_author ON prompt_critiques(author_id);

-- The shape of `comment_reports`. A report goes to the entry's author.
CREATE TABLE prompt_critique_reports (
    id          TEXT PRIMARY KEY NOT NULL,
    critique_id TEXT NOT NULL REFERENCES prompt_critiques(id) ON DELETE CASCADE,
    reporter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason      TEXT NOT NULL,
    detail      TEXT,
    created_at  TEXT NOT NULL,
    UNIQUE (critique_id, reporter_id)
);

-- Something that happened that a writer should hear about. Generic on purpose:
-- `kind` is an open set described by a registry in `domain::inbox`, and the
-- subject and its context (a critique, and the entry it is on) say what to
-- delete the item with.
CREATE TABLE inbox_items (
    id           TEXT PRIMARY KEY NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind         TEXT NOT NULL,
    subject_type TEXT NOT NULL,
    subject_id   TEXT NOT NULL,
    context_type TEXT,
    context_id   TEXT,
    actor_id     TEXT REFERENCES users(id) ON DELETE SET NULL,
    -- JSON the registry reads to describe the item and link to it.
    detail       TEXT,
    created_at   TEXT NOT NULL,
    read_at      TEXT
);
CREATE INDEX idx_inbox_items_user ON inbox_items(user_id, created_at);
CREATE INDEX idx_inbox_items_subject ON inbox_items(subject_type, subject_id);
CREATE INDEX idx_inbox_items_context ON inbox_items(context_type, context_id);

-- Yesterday, summed up, pinned at the top of the inbox until read.
CREATE TABLE inbox_digests (
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day          TEXT NOT NULL,
    summary_json TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    read_at      TEXT,
    PRIMARY KEY (user_id, day)
);

-- One prompt email per writer per day, recorded in the same transaction that
-- queues it.
CREATE TABLE prompt_email_sends (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day     TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    PRIMARY KEY (user_id, day)
);
