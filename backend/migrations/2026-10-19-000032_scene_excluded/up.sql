-- A scene the writer keeps in the book's outline but not in the book: notes
-- in scene form, an alternate take, a cut kept for reference. It is not
-- printed in an export, not frozen into a version, and not counted in the
-- book's word count.
ALTER TABLE scenes ADD COLUMN excluded BOOLEAN NOT NULL DEFAULT 0;
