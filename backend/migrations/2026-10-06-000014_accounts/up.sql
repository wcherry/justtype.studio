-- Proof that the address on an account belongs to the person using it.
-- Invitations to read and to co-write are addressed to an email, so until an
-- address is proved, anyone could register it and collect what was sent to it.
ALTER TABLE users ADD COLUMN email_verified_at TEXT;

-- Time-based one-time passwords. The secret is written at setup and only
-- counts once a code from it has been checked, so a half-finished setup never
-- locks anyone out.
ALTER TABLE users ADD COLUMN totp_secret TEXT;
ALTER TABLE users ADD COLUMN totp_enabled_at TEXT;
-- The last time step a code was accepted for, so one code works once.
ALTER TABLE users ADD COLUMN totp_last_step INTEGER;

-- One row per signed-in device. Refresh tokens rotate underneath it; the
-- session is what a person sees in their list and revokes, and every access
-- token names the session it came from so revoking one takes effect at once
-- rather than when the token runs out.
CREATE TABLE sessions (
    id           TEXT PRIMARY KEY NOT NULL,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent   TEXT,
    ip           TEXT,
    created_at   TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    revoked_at   TEXT
);
CREATE INDEX idx_sessions_user ON sessions(user_id);
-- No foreign key: SQLite cannot drop a column that carries one, which would
-- leave this migration impossible to roll back. Sessions are only ever removed
-- with their user, and the user's tokens go with them.
ALTER TABLE refresh_tokens ADD COLUMN session_id TEXT;

-- Single-use links sent by email: proving an address, resetting a password.
-- Only a digest is kept, as with refresh tokens.
CREATE TABLE email_tokens (
    id         TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    purpose    TEXT NOT NULL,
    email      TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    used_at    TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_email_tokens_user ON email_tokens(user_id, purpose);

-- For when the phone with the authenticator on it is lost. Digests only.
CREATE TABLE recovery_codes (
    id        TEXT PRIMARY KEY NOT NULL,
    user_id   TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at   TEXT
);
CREATE INDEX idx_recovery_codes_user ON recovery_codes(user_id);
