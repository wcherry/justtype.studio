-- Back matter: a chapter is a chapter, front matter or back matter.
ALTER TABLE chapters ADD COLUMN role TEXT NOT NULL DEFAULT 'chapter';

-- Reusable across books, like the contact details.
ALTER TABLE users ADD COLUMN about_author TEXT;

-- What export needs to know about a book beyond its title and byline. A table
-- of its own rather than more columns on `works`, which is already wide.
CREATE TABLE work_metadata (
    work_id         TEXT PRIMARY KEY NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    language        TEXT,
    text_direction  TEXT,
    description     TEXT,
    publisher       TEXT,
    published       TEXT,
    identifier      TEXT,
    keywords        TEXT,
    contributors    TEXT,
    cover_mime      TEXT,
    cover_bytes     INTEGER,
    cover_width     INTEGER,
    cover_height    INTEGER,
    updated_at      TEXT NOT NULL
);

-- Presentation, not content: options minus the selection and the metadata.
CREATE TABLE export_profiles (
    id          TEXT PRIMARY KEY NOT NULL,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    options     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX idx_export_profiles_user ON export_profiles(user_id, name);

-- The options last used, per work per format.
CREATE TABLE export_last_options (
    work_id     TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    format      TEXT NOT NULL,
    options     TEXT NOT NULL,
    used_at     TEXT NOT NULL,
    PRIMARY KEY (work_id, format)
);

-- What was sent, and when. Version exports can be built again from the frozen
-- text; draft exports keep the record only.
CREATE TABLE export_history (
    id            TEXT PRIMARY KEY NOT NULL,
    work_id       TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    user_id       TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    version_id    TEXT,
    format        TEXT NOT NULL,
    profile_name  TEXT,
    options_hash  TEXT NOT NULL,
    options       TEXT NOT NULL,
    filename      TEXT NOT NULL,
    byte_size     INTEGER NOT NULL,
    content_hash  TEXT NOT NULL,
    created_at    TEXT NOT NULL
);
CREATE INDEX idx_export_history_work ON export_history(work_id, created_at);

-- A snapshot freezes the role too, so a version exports the way it was sent.
ALTER TABLE version_entries ADD COLUMN chapter_role TEXT NOT NULL DEFAULT 'chapter';
