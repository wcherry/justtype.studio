-- Where each writer was in each book: the scene, and the selection in it, so
-- opening the book again — on any device — puts them back there. Per writer,
-- since collaborators on one book are each somewhere different in it.
CREATE TABLE writing_places (
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    work_id    TEXT NOT NULL REFERENCES works(id) ON DELETE CASCADE,
    scene_id   TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    sel_from   INTEGER NOT NULL,
    sel_to     INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (user_id, work_id)
);
