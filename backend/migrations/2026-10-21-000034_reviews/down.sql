DROP INDEX idx_publication_invites_user;
ALTER TABLE publication_invites DROP COLUMN user_id;
ALTER TABLE publications DROP COLUMN revoked_at;
