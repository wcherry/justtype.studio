DROP TABLE IF EXISTS version_tags;
ALTER TABLE works DROP COLUMN pinned_version_id;
ALTER TABLE versions DROP COLUMN scope;
ALTER TABLE versions DROP COLUMN scope_ref_id;
