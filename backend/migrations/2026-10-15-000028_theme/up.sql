-- How the app looks for this writer: `system` follows the device's light or
-- dark setting; the rest are fixed. Kept with the account, so it follows the
-- writer to every device, unlike the measure and type size of the page,
-- which belong to the screen in front of them.
ALTER TABLE users ADD COLUMN theme TEXT NOT NULL DEFAULT 'system';
