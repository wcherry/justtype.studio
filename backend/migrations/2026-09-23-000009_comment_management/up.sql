-- A note becomes work: the author's own list of what to change.
--
-- Separate from tracked_changes, which proposes an exact edit to an exact
-- range. A revision task is the other kind: "chapter four needs to be colder",
-- which no diff can express and which outlives the scene it started in.
CREATE TABLE revision_tasks (
    id         TEXT PRIMARY KEY NOT NULL,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    -- Where the work is, when it is anywhere in particular.
    scene_id   TEXT REFERENCES scenes(id) ON DELETE SET NULL,
    -- The note it came from. Kept so the author can read the whole thread
    -- again, and nulled rather than cascaded if that comment is deleted: the
    -- task is the author's now.
    comment_id TEXT REFERENCES comments(id) ON DELETE SET NULL,
    body       TEXT NOT NULL,
    state      TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    done_at    TEXT
);
CREATE INDEX idx_revision_tasks_work ON revision_tasks(work_id, state);
-- One task per comment: turning the same note into work twice is a slip, not
-- an intention.
CREATE UNIQUE INDEX idx_revision_tasks_comment ON revision_tasks(comment_id)
    WHERE comment_id IS NOT NULL;
