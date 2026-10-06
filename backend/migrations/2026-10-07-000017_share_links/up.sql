-- Unlisted publications are opened with a signed, expiring link rather than
-- with their id. The signature covers this secret, so replacing it revokes
-- every link to the publication at once. Empty until the first link is made.
ALTER TABLE publications ADD COLUMN link_secret TEXT;
