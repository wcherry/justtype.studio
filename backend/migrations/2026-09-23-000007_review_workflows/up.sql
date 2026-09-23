-- Review workflows: who may read a publication, for how long, and how far they
-- have got.

-- A cap on how many people may comment. Beta readers are a scarce resource and
-- twenty overlapping opinions are worse than five considered ones.
ALTER TABLE publications ADD COLUMN max_reviewers INTEGER;

-- Named invitations, by email rather than by account: the person a writer wants
-- to read their novel usually has an email address and not yet an account here.
CREATE TABLE publication_invites (
    id             TEXT PRIMARY KEY NOT NULL,
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    -- Lowercased on the way in, so an invitation matches however the reader
    -- capitalises their own address when they sign up.
    email          TEXT NOT NULL,
    invited_by     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Set the first time the invited reader opens it, which is how an author
    -- can tell who has actually turned up.
    accepted_at    TEXT,
    accepted_by    TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL,
    UNIQUE (publication_id, email)
);
CREATE INDEX idx_publication_invites_email ON publication_invites(email);

-- A standing list of readers, so inviting "my beta readers" is one action
-- rather than eleven.
CREATE TABLE reader_circles (
    id         TEXT PRIMARY KEY NOT NULL,
    owner_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_reader_circles_owner ON reader_circles(owner_id);

CREATE TABLE circle_members (
    id         TEXT PRIMARY KEY NOT NULL,
    circle_id  TEXT NOT NULL REFERENCES reader_circles(id) ON DELETE CASCADE,
    email      TEXT NOT NULL,
    name       TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (circle_id, email)
);

-- Where a reader got to. One row per reader per publication: the question is
-- "how far did they get", not "every page they turned".
CREATE TABLE reading_progress (
    id               TEXT PRIMARY KEY NOT NULL,
    publication_id   TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    user_id          TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Where they are now, so their next visit opens where they stopped.
    scene_id         TEXT NOT NULL,
    -- The furthest they ever reached, which is the interesting one: a reader
    -- who gave up in chapter three and later skimmed the ending has still
    -- stopped in chapter three.
    furthest_index   INTEGER NOT NULL DEFAULT 0,
    scenes_total     INTEGER NOT NULL DEFAULT 0,
    finished_at      TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    UNIQUE (publication_id, user_id)
);
CREATE INDEX idx_reading_progress_publication ON reading_progress(publication_id);
