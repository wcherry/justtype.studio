ALTER TABLE notebook_entries RENAME TO bible_entries;
DROP INDEX idx_notebook_entries_work;
CREATE INDEX idx_bible_entries_work ON bible_entries(work_id, kind, name);

CREATE TABLE scene_bible_links (
    id         TEXT PRIMARY KEY NOT NULL,
    scene_id   TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    entry_id   TEXT NOT NULL REFERENCES bible_entries(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE (scene_id, entry_id)
);
INSERT INTO scene_bible_links (id, scene_id, entry_id, created_at)
    SELECT id, scene_id, entry_id, created_at FROM scene_notebook_links;
DROP TABLE scene_notebook_links;
CREATE INDEX idx_scene_bible_links_entry ON scene_bible_links(entry_id);
