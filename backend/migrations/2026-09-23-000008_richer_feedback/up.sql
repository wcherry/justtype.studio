-- Richer feedback: the signals a reader will actually give.
--
-- Most readers will not write a paragraph about your paragraph. They will tell
-- you, in one tap, that they got lost here and loved that — if the cost of
-- saying so is one tap.

-- A reaction is anchored to a *block*, not to an arbitrary selection. Two
-- readers never highlight quite the same words, so span-anchored reactions
-- could not be added up; paragraph-anchored ones can, and the sum is the whole
-- point. One row per reader per block: tapping a different face replaces the
-- old one, so the tally is people rather than taps.
CREATE TABLE reactions (
    id             TEXT PRIMARY KEY NOT NULL,
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    user_id        TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scene_id       TEXT NOT NULL,
    block_index    INTEGER NOT NULL,
    kind           TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    UNIQUE (publication_id, user_id, scene_id, block_index)
);
CREATE INDEX idx_reactions_publication ON reactions(publication_id, scene_id, block_index);

-- The author's own questions, asked of every reader. A `chapter`-scoped
-- question is asked once per chapter — one prompt, twelve answers, which is
-- what makes "does the middle sag" answerable instead of rhetorical.
CREATE TABLE review_questions (
    id             TEXT PRIMARY KEY NOT NULL,
    publication_id TEXT NOT NULL REFERENCES publications(id) ON DELETE CASCADE,
    prompt         TEXT NOT NULL,
    -- 'rating' (1-5) or 'text'.
    kind           TEXT NOT NULL,
    -- 'work' (asked once) or 'chapter' (asked of each chapter in the slice).
    scope          TEXT NOT NULL,
    position       INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
CREATE INDEX idx_review_questions_publication ON review_questions(publication_id, position);

CREATE TABLE review_answers (
    id          TEXT PRIMARY KEY NOT NULL,
    question_id TEXT NOT NULL REFERENCES review_questions(id) ON DELETE CASCADE,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- '' for a work-scoped question. Not NULL, because SQLite counts two NULLs
    -- as distinct in a UNIQUE index, and the constraint below would then let
    -- one reader answer the same question any number of times.
    chapter_id  TEXT NOT NULL DEFAULT '',
    rating      INTEGER,
    body        TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (question_id, user_id, chapter_id)
);
CREATE INDEX idx_review_answers_question ON review_answers(question_id);
