-- A genre to browse by. Nullable: plenty of writers do not know yet, and a
-- work with no genre is still on offer — it just does not turn up when someone
-- narrows the feed to one.
ALTER TABLE works ADD COLUMN genre TEXT;
CREATE INDEX idx_works_genre ON works(genre);
