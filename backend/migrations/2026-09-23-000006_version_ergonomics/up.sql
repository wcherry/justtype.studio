-- Making a shelf of snapshots usable: naming them, finding them again, and
-- taking one of less than the whole book.

-- Free-form labels on a snapshot. A table rather than a column, because the
-- interesting question is "which version did I submit" — asked of the tag, not
-- of the version.
CREATE TABLE version_tags (
    id         TEXT PRIMARY KEY NOT NULL,
    version_id TEXT NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
    tag        TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (version_id, tag)
);
CREATE INDEX idx_version_tags_tag ON version_tags(tag);

-- The one snapshot a writer wants to find without looking: what they last sent
-- out. Singular by nature, so a pointer rather than a tag.
ALTER TABLE works ADD COLUMN pinned_version_id TEXT;

-- A snapshot of less than the whole work — checkpointing one chapter before
-- rewriting it, without implying anything about the rest.
ALTER TABLE versions ADD COLUMN scope TEXT NOT NULL DEFAULT 'work';
ALTER TABLE versions ADD COLUMN scope_ref_id TEXT;
