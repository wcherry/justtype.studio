-- The writing assistant sends text to the same model provider summaries do,
-- but for a different purpose — rewriting and continuing, not describing — so
-- it is a separate yes, given separately.
ALTER TABLE users ADD COLUMN assistant_allowed_at TEXT;

-- Words the writer has told the spellchecker are right: names, places, the
-- language of the book. Per writer rather than per book, since a writer's
-- vocabulary follows them; kept here so it follows them onto every device.
CREATE TABLE dictionary_words (
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    word       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, word)
);
