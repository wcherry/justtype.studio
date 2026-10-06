-- Summaries are written by a model that runs outside this service, so the
-- text goes to a third party. Nobody's manuscript leaves without that person
-- having said yes, once, knowing where it goes.
ALTER TABLE users ADD COLUMN summaries_allowed_at TEXT;

-- What was written, kept by what it was written from: asking again for text
-- that has not changed costs nothing and gives the same answer.
CREATE TABLE summaries (
    id            TEXT PRIMARY KEY NOT NULL,
    work_id       TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    -- `scene`, `chapter` or `work`.
    target_kind   TEXT NOT NULL,
    target_id     TEXT NOT NULL,
    -- A digest of the exact text and instructions sent, model included.
    input_hash    TEXT NOT NULL,
    summary       TEXT NOT NULL,
    model         TEXT NOT NULL,
    input_tokens  INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    created_by    TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at    TEXT NOT NULL,
    UNIQUE (target_kind, target_id, input_hash)
);
CREATE INDEX idx_summaries_work ON summaries(work_id);
