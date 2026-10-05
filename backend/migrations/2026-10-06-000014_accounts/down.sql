DROP TABLE recovery_codes;
DROP TABLE email_tokens;
ALTER TABLE refresh_tokens DROP COLUMN session_id;
DROP TABLE sessions;
ALTER TABLE users DROP COLUMN totp_last_step;
ALTER TABLE users DROP COLUMN totp_enabled_at;
ALTER TABLE users DROP COLUMN totp_secret;
ALTER TABLE users DROP COLUMN email_verified_at;
