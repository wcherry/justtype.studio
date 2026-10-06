-- Full-text search, with SQLite's own FTS5 (compiled into the bundled build).
--
-- The indexes are refreshed when someone searches, not when something is
-- written: a search first re-indexes whatever has changed since the last one,
-- found by comparing what the index was built from (the state tables) with
-- what is there now. A scene is written from a dozen places — saves, splits,
-- merges, restores, accepted changes — and an index each of them had to
-- remember to update would be one that drifts.

-- A writer's own working text, searched within one work.
CREATE VIRTUAL TABLE scene_search USING fts5(
    scene_id UNINDEXED,
    work_id UNINDEXED,
    title,
    body,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE TABLE scene_search_state (
    scene_id     TEXT PRIMARY KEY NOT NULL,
    work_id      TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    title        TEXT NOT NULL
);
CREATE INDEX idx_scene_search_state_work ON scene_search_state(work_id);

-- What is on offer, in two tables. FTS5 scores a match against the whole
-- row's length, so a title and a novel's text in one row would bury every
-- novel's title under its own eighty thousand words. The short fields rank
-- first; the published text is searched second.
CREATE VIRTUAL TABLE publication_search USING fts5(
    publication_id UNINDEXED,
    title,
    author,
    prompt,
    synopsis,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE publication_text USING fts5(
    publication_id UNINDEXED,
    body,
    tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE TABLE publication_search_state (
    publication_id TEXT PRIMARY KEY NOT NULL,
    -- A digest of the fields above that can change; the text cannot.
    signature      TEXT NOT NULL
);
