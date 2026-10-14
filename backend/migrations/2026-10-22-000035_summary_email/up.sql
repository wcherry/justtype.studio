-- The summary email: what the writer wrote, what is still on their to-do
-- lists, and what reviewers said, every morning or every Monday morning.
--
-- No row is the same as `none`. `often` is `none`, `daily` or `weekly`; a row
-- is kept when it is turned off so its token, in the footer of every email
-- already sent, still works. The timezone is the writer's own, sent by the
-- device they chose it on, because their words are counted against their
-- local day and the email should arrive in their morning. `updated_at` is when
-- they last changed it, so turning it on in the afternoon does not send a
-- summary for a morning that had already gone.
CREATE TABLE summary_email_settings (
    user_id    TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    often      TEXT NOT NULL DEFAULT 'none',
    timezone   TEXT NOT NULL,
    token      TEXT NOT NULL UNIQUE,
    updated_at TEXT NOT NULL
);

-- One summary per writer per period (`daily:2026-10-21`, `weekly:2026-10-12`),
-- recorded in the same transaction that queues it. A period with nothing in
-- it is recorded too, with `queued` false, so it is not looked at again.
CREATE TABLE summary_email_sends (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period  TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    queued  BOOLEAN NOT NULL,
    PRIMARY KEY (user_id, period)
);
