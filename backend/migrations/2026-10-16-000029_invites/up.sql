-- Invitations to join, one row per writer per address they typed. The row is
-- written whether or not mail went: an address that already has an account is
-- not mailed, but it counts the same, so neither the reply nor the daily limit
-- tells the inviter who is already here.
CREATE TABLE site_invites (
    id         TEXT PRIMARY KEY NOT NULL,
    inviter_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    email      TEXT NOT NULL,
    mailed     INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (inviter_id, email)
);
CREATE INDEX site_invites_by_email ON site_invites (email, created_at);
CREATE INDEX site_invites_by_inviter ON site_invites (inviter_id, created_at);
