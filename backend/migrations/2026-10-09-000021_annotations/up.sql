-- A reader's or writer's own marks on the text: highlights and bookmarks.
-- Private to the person who made them; nobody else is ever shown them, and
-- they never touch the manuscript. On a publication the positions are exact,
-- because a snapshot does not change. On the working draft they are where the
-- text was, and `quote` is what finds it again after the text has moved.
CREATE TABLE annotations (
    id             TEXT PRIMARY KEY NOT NULL,
    user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id        TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    scene_id       TEXT NOT NULL,
    -- NULL for the working draft.
    publication_id TEXT REFERENCES publications(id) ON DELETE CASCADE,
    -- `highlight` or `bookmark`.
    kind           TEXT NOT NULL,
    from_pos       INTEGER NOT NULL,
    to_pos         INTEGER NOT NULL,
    quote          TEXT NOT NULL,
    -- A bookmark's name, or a line about a highlight.
    note           TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
CREATE INDEX idx_annotations_draft ON annotations(user_id, work_id) WHERE publication_id IS NULL;
CREATE INDEX idx_annotations_publication ON annotations(user_id, publication_id);

-- A to-do written against a passage keeps the passage.
ALTER TABLE revision_tasks ADD COLUMN quote TEXT;
