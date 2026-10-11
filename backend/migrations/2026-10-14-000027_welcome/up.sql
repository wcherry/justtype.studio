-- When the welcome email was queued. It goes once, the first time an address
-- is proved; confirming again, or a new address later, never sends another.
ALTER TABLE users ADD COLUMN welcomed_at TEXT;

-- Everyone already confirmed is past the moment a welcome is for.
UPDATE users SET welcomed_at = email_verified_at WHERE email_verified_at IS NOT NULL;
