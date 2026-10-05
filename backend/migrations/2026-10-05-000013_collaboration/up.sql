-- Co-authors. Addressed to an email rather than an account, like reader
-- invitations: the person a writer wants on the book may not have signed up
-- yet. `user_id` is filled in the first time they open it.
--
-- The work's creator is not a row here. `works.owner_id` already says who they
-- are, and keeping them out means nothing in this table can remove them.
CREATE TABLE work_collaborators (
    id             TEXT PRIMARY KEY NOT NULL,
    work_id        TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    email          TEXT NOT NULL,
    user_id        TEXT REFERENCES users(id) ON DELETE SET NULL,
    role           TEXT NOT NULL,
    -- 1 when the collaborator may change only the chapters listed in
    -- collaborator_chapters. A flag rather than "no rows means everywhere":
    -- deleting the last granted chapter must leave them nowhere, not
    -- everywhere.
    chapter_scoped INTEGER NOT NULL DEFAULT 0,
    -- Kept if the inviter leaves: the person they brought in stays on the book.
    invited_by     TEXT REFERENCES users(id) ON DELETE SET NULL,
    accepted_at    TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE (work_id, email)
);
CREATE INDEX idx_work_collaborators_email ON work_collaborators(email);

CREATE TABLE collaborator_chapters (
    id              TEXT PRIMARY KEY NOT NULL,
    collaborator_id TEXT NOT NULL REFERENCES work_collaborators(id) ON DELETE CASCADE,
    chapter_id      TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    UNIQUE (collaborator_id, chapter_id)
);

-- What happened to a work, and who did it. Written by the handlers as they
-- act, because a derived feed cannot say who deleted a chapter: the row that
-- would have said so is gone.
CREATE TABLE work_activity (
    id         TEXT PRIMARY KEY NOT NULL,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    actor_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,
    subject_id TEXT,
    -- The chapter, scene or version title as it was at the time, so the feed
    -- still reads after the thing is renamed or gone.
    subject    TEXT NOT NULL,
    -- How many times this entry was folded together: a sitting of saves to
    -- one scene is one line, not two hundred.
    times      INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_work_activity_work ON work_activity(work_id, updated_at);
