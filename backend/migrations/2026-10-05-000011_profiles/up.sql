-- Following an author: one row per pair, and the follower decides. There is no
-- approval step — what a follow buys is a feed of work the author has already
-- chosen to show, so it reveals nothing the author did not offer.
CREATE TABLE follows (
    id          TEXT PRIMARY KEY NOT NULL,
    follower_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    followee_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at  TEXT NOT NULL,
    UNIQUE (follower_id, followee_id),
    CHECK (follower_id <> followee_id)
);
CREATE INDEX idx_follows_followee ON follows(followee_id);
