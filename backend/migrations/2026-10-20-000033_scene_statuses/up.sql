-- The writer's own scene statuses for a book, beside the four every book has
-- (outline, drafting, revised, done). A scene whose status is one of these
-- holds the row's id in `scenes.status`; there is no foreign key because the
-- column holds the built-in names too, so removing one moves its scenes back
-- to drafting in the handler.
CREATE TABLE scene_statuses (
    id         TEXT PRIMARY KEY NOT NULL,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    label      TEXT NOT NULL,
    -- `#rrggbb`: the dot before the scene's title in the outline.
    color      TEXT NOT NULL,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_scene_statuses_work ON scene_statuses(work_id, position);
