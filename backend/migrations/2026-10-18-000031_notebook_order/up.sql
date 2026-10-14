-- The writer's own order for the notebook, within each kind: the people who
-- matter most at the top, not whoever sorts first. Existing entries keep the
-- order they were shown in until now, alphabetical by name.
ALTER TABLE notebook_entries ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

UPDATE notebook_entries SET position = (
    SELECT COUNT(*) FROM notebook_entries AS before
    WHERE before.work_id = notebook_entries.work_id
      AND before.kind = notebook_entries.kind
      AND (before.name < notebook_entries.name
           OR (before.name = notebook_entries.name AND before.id < notebook_entries.id))
);
