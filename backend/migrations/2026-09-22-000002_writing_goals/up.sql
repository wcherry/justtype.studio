-- Writing goals and the daily record a streak is counted from.

-- One row per writer per day they wrote. `day` is the writer's *local* date,
-- supplied by the client: only it knows what day it is where the writer is, and
-- a UTC day would break a streak for anyone writing late in the evening west of
-- Greenwich.
CREATE TABLE writing_days (
    id         TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day        TEXT NOT NULL,                    -- YYYY-MM-DD, local to the writer
    -- Net, so a day spent cutting reads as the negative number it is. A day
    -- counts toward a streak because the writer showed up, not because the
    -- total went up.
    net_words  INTEGER NOT NULL DEFAULT 0,
    saves      INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (user_id, day)
);
CREATE INDEX idx_writing_days_user ON writing_days(user_id, day);

-- A daily target, in words. NULL means the writer has not set one.
ALTER TABLE users ADD COLUMN daily_word_goal INTEGER;

-- A per-scene target, for writers who plan at that level.
ALTER TABLE scenes ADD COLUMN target_words INTEGER;
