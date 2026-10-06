-- Sketches: a bible entry with named fields — what she wants, what she fears,
-- what the place smells of — seeded from a template when it is made. JSON, as
-- an ordered list of {key, label, value}: the writer can add their own fields
-- and reorder nothing by accident, and nothing ever queries inside it.
ALTER TABLE bible_entries ADD COLUMN fields TEXT;
-- Which template it was started from, for the label on the card. Not a
-- reference: templates are built in and may be renamed.
ALTER TABLE bible_entries ADD COLUMN template_id TEXT;
