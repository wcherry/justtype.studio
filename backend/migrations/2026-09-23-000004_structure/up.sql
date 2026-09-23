-- Structure tools: scene metadata to filter by, and a story bible to keep
-- continuity in one place instead of in the writer's head.

-- Free text rather than a date, because fiction rarely runs on the Gregorian
-- calendar: "the third day of the siege" is a timeline position too.
ALTER TABLE scenes ADD COLUMN location TEXT;
ALTER TABLE scenes ADD COLUMN story_date TEXT;

-- Characters, places, and the continuity notes that do not belong to either.
CREATE TABLE bible_entries (
    id         TEXT PRIMARY KEY NOT NULL,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL,          -- character | place | note
    name       TEXT NOT NULL,
    -- One line for the card; the body is the long form.
    summary    TEXT,
    body       TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_bible_entries_work ON bible_entries(work_id, kind, name);

-- Which entries appear in which scenes. A join table rather than a list on
-- either side, because the question gets asked from both directions: "who is in
-- this scene" and "which scenes is she in".
CREATE TABLE scene_bible_links (
    id         TEXT PRIMARY KEY NOT NULL,
    scene_id   TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    entry_id   TEXT NOT NULL REFERENCES bible_entries(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE (scene_id, entry_id)
);
CREATE INDEX idx_scene_bible_links_entry ON scene_bible_links(entry_id);
