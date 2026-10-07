-- The story bible is the notebook now: the same entries under a plainer name.
ALTER TABLE bible_entries RENAME TO notebook_entries;
DROP INDEX idx_bible_entries_work;
CREATE INDEX idx_notebook_entries_work ON notebook_entries(work_id, kind, name);

-- Rebuilt rather than renamed: whether a rename rewrites the reference to the
-- entries table depends on how SQLite was built, and this way it always does.
CREATE TABLE scene_notebook_links (
    id         TEXT PRIMARY KEY NOT NULL,
    scene_id   TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    entry_id   TEXT NOT NULL REFERENCES notebook_entries(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE (scene_id, entry_id)
);
INSERT INTO scene_notebook_links (id, scene_id, entry_id, created_at)
    SELECT id, scene_id, entry_id, created_at FROM scene_bible_links;
DROP TABLE scene_bible_links;
CREATE INDEX idx_scene_notebook_links_entry ON scene_notebook_links(entry_id);
