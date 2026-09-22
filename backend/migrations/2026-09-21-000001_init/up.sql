-- Scriptorium initial schema.
-- Convention: ids are UUIDv4 strings, timestamps are ISO-8601 UTC strings
-- (lexicographically sortable), manuscript content lives on disk, not here.

CREATE TABLE users (
    id            TEXT PRIMARY KEY NOT NULL,
    email         TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    bio           TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE refresh_tokens (
    id         TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    revoked_at TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_refresh_tokens_user ON refresh_tokens(user_id);

-- Structural templates (novel / three-act / short story / built-ins and user-made).
CREATE TABLE templates (
    id             TEXT PRIMARY KEY NOT NULL,
    owner_id       TEXT REFERENCES users(id) ON DELETE CASCADE, -- NULL = built-in
    name           TEXT NOT NULL,
    kind           TEXT NOT NULL,
    description    TEXT NOT NULL,
    structure_json TEXT NOT NULL,
    created_at     TEXT NOT NULL
);
CREATE INDEX idx_templates_owner ON templates(owner_id);

CREATE TABLE works (
    id                 TEXT PRIMARY KEY NOT NULL,
    owner_id           TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title              TEXT NOT NULL,
    subtitle           TEXT,
    kind               TEXT NOT NULL,              -- novel | novella | short_story | serial
    synopsis           TEXT,
    template_id        TEXT REFERENCES templates(id) ON DELETE SET NULL,
    status             TEXT NOT NULL,              -- draft | published | final | archived
    current_version_id TEXT,
    word_count         INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL
);
CREATE INDEX idx_works_owner ON works(owner_id);

CREATE TABLE chapters (
    id         TEXT PRIMARY KEY NOT NULL,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    summary    TEXT,
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_chapters_work ON chapters(work_id, position);

CREATE TABLE scenes (
    id           TEXT PRIMARY KEY NOT NULL,
    work_id      TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    chapter_id   TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    summary      TEXT,
    pov          TEXT,
    status       TEXT NOT NULL,        -- outline | drafting | revised | done
    position     INTEGER NOT NULL,
    word_count   INTEGER NOT NULL DEFAULT 0,
    content_hash TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE INDEX idx_scenes_chapter ON scenes(chapter_id, position);
CREATE INDEX idx_scenes_work ON scenes(work_id);

-- Autosave history for the mutable working copy of a scene.
CREATE TABLE scene_revisions (
    id           TEXT PRIMARY KEY NOT NULL,
    scene_id     TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    work_id      TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    author_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    word_count   INTEGER NOT NULL,
    created_at   TEXT NOT NULL
);
CREATE INDEX idx_scene_revisions_scene ON scene_revisions(scene_id, created_at);

-- Word-style tracked changes: proposed edits pending accept/reject.
CREATE TABLE tracked_changes (
    id          TEXT PRIMARY KEY NOT NULL,
    scene_id    TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    work_id     TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    author_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,        -- insert | delete | replace | format
    anchor_from INTEGER NOT NULL,
    anchor_to   INTEGER NOT NULL,
    quote       TEXT,
    payload     TEXT NOT NULL,        -- JSON: replacement slice / mark spec
    note        TEXT,
    state       TEXT NOT NULL,        -- pending | accepted | rejected
    resolved_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    resolved_at TEXT,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_tracked_changes_scene ON tracked_changes(scene_id, state);

-- Immutable snapshot of a whole work.
CREATE TABLE versions (
    id                TEXT PRIMARY KEY NOT NULL,
    work_id           TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    number            INTEGER NOT NULL,
    label             TEXT NOT NULL,
    state             TEXT NOT NULL,   -- draft | published | final
    parent_version_id TEXT REFERENCES versions(id) ON DELETE SET NULL,
    created_by        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    notes             TEXT,
    word_count        INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    published_at      TEXT,
    finalized_at      TEXT
);
CREATE UNIQUE INDEX idx_versions_work_number ON versions(work_id, number);

-- Flattened manifest of one snapshot, so diffs never need to open the blobs.
CREATE TABLE version_entries (
    id               TEXT PRIMARY KEY NOT NULL,
    version_id       TEXT NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    chapter_id       TEXT NOT NULL,
    chapter_title    TEXT NOT NULL,
    chapter_position INTEGER NOT NULL,
    scene_id         TEXT NOT NULL,
    scene_title      TEXT NOT NULL,
    scene_position   INTEGER NOT NULL,
    content_hash     TEXT NOT NULL,
    word_count       INTEGER NOT NULL
);
CREATE INDEX idx_version_entries_version ON version_entries(version_id, chapter_position, scene_position);

-- A version offered to reviewers.
CREATE TABLE publications (
    id            TEXT PRIMARY KEY NOT NULL,
    work_id       TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    version_id    TEXT NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    visibility    TEXT NOT NULL,       -- unlisted | platform | public
    scope         TEXT NOT NULL,       -- work | chapter | scene
    scope_ref_id  TEXT,
    review_prompt TEXT,
    opens_at      TEXT,
    closes_at     TEXT,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX idx_publications_work ON publications(work_id);
CREATE INDEX idx_publications_feed ON publications(visibility, created_at);

CREATE TABLE comments (
    id             TEXT PRIMARY KEY NOT NULL,
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    version_id     TEXT NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    scene_id       TEXT,
    parent_id      TEXT REFERENCES comments(id) ON DELETE CASCADE,
    author_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body           TEXT NOT NULL,
    anchor         TEXT,               -- JSON {from,to} into the snapshot doc
    anchor_quote   TEXT,
    category       TEXT NOT NULL,      -- general | praise | plot | pacing | prose | character | typo
    state          TEXT NOT NULL,      -- open | resolved | deleted
    score          INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
CREATE INDEX idx_comments_publication ON comments(publication_id, created_at);
CREATE INDEX idx_comments_scene ON comments(scene_id);
CREATE INDEX idx_comments_parent ON comments(parent_id);

CREATE TABLE comment_votes (
    id         TEXT PRIMARY KEY NOT NULL,
    comment_id TEXT NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value      INTEGER NOT NULL,       -- -1 | 1
    created_at TEXT NOT NULL,
    UNIQUE (comment_id, user_id)
);
CREATE INDEX idx_comment_votes_comment ON comment_votes(comment_id);
