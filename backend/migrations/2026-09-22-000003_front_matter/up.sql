-- Front matter: what a title page and a submission header need.

-- Contact details belong to the writer, not to any one work, and a submission
-- in standard manuscript format has to carry them.
ALTER TABLE users ADD COLUMN legal_name TEXT;
ALTER TABLE users ADD COLUMN mailing_address TEXT;
ALTER TABLE users ADD COLUMN phone TEXT;

-- Per work, because a writer may publish different books under different names.
ALTER TABLE works ADD COLUMN pen_name TEXT;
-- The running header wants something short; a long title wraps and ruins it.
ALTER TABLE works ADD COLUMN short_title TEXT;
ALTER TABLE works ADD COLUMN dedication TEXT;
ALTER TABLE works ADD COLUMN epigraph TEXT;
ALTER TABLE works ADD COLUMN epigraph_source TEXT;
ALTER TABLE works ADD COLUMN copyright_line TEXT;
