-- The document state a proposal was made against.
--
-- Without it there is no way to tell whether a change's positions still mean
-- what they meant when it was written, so accepting one was a guess. With it,
-- an unchanged document applies exactly and a changed one re-anchors by the
-- quoted text. NULL means an older row whose base is unknown: those always
-- re-anchor.
ALTER TABLE tracked_changes ADD COLUMN base_hash TEXT;
