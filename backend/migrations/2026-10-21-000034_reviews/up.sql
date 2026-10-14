-- Reviews: a scene, chapter, part or the whole book put in front of readers
-- from the outline. A review is a publication; these are what it needed.
--
-- Revoking one closes it to its readers without losing what they wrote: the
-- author keeps every comment, and can open it again.
ALTER TABLE publications ADD COLUMN revoked_at TEXT;

-- A reader picked by name rather than by address. `email` still holds their
-- address so one person invited both ways is one invitation, but it is never
-- shown back to the author who picked them: they chose a name, not an email.
ALTER TABLE publication_invites ADD COLUMN user_id TEXT REFERENCES users(id) ON DELETE CASCADE;
CREATE INDEX idx_publication_invites_user ON publication_invites(user_id);
