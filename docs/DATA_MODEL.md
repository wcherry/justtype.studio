# Data model

Ids are UUIDv4 strings. Timestamps are ISO-8601 UTC strings, which SQLite sorts
correctly as text and every client parses natively.

```
users ──┬─< works ──┬─< chapters ──< scenes ──┬─< scene_revisions
        │           │                          └─< tracked_changes
        │           └─< versions ──┬─< version_entries
        │                          └─< publications ──< comments ──< comment_votes
        ├─< refresh_tokens
        └─< templates (owner_id NULL = built-in)
```

## Tables

| Table | Holds | Notes |
|---|---|---|
| `users` | Account and profile | Argon2id hash; never serialised to clients |
| `refresh_tokens` | Session continuation | SHA-256 digest only; rotated on every use |
| `templates` | Structural skeletons | `owner_id IS NULL` means built-in |
| `works` | A novel, novella, short story or serial | `word_count` recomputed on every scene write |
| `chapters` | Ordered divisions of a work | `position` is dense and zero-based |
| `scenes` | The unit of writing | `content_hash` detects real changes; body is on disk |
| `scene_revisions` | Autosave history | One row per checkpoint; body is on disk |
| `tracked_changes` | Proposed edits | `pending → accepted / rejected` |
| `versions` | Immutable snapshots | `draft → published → final`, `number` unique per work |
| `version_entries` | Flat manifest of one snapshot | Lets diffs run without opening any file |
| `publications` | An offer of a version to readers | Visibility and scope live here |
| `comments` | Reviewer feedback | Anchored, threaded, categorised; soft-deleted |
| `comment_votes` | One vote per person per comment | Score recomputed, never incremented |
| `follows` | Who follows whom | One row per pair; nobody follows themselves |
| `work_collaborators` | Co-authors on a work | Addressed by email; the creator is never a row |
| `collaborator_chapters` | The chapters a collaborator was handed | Only read when `chapter_scoped` is set, so none means nowhere |
| `work_activity` | What happened to a work, and who did it | Written as it happens; edits fold into one line per sitting |

## Lifecycles

**Work** — `draft` → `published` (first version published) → `final` (a version
marked final). `archived` is a separate, deliberate author action.

**Version** — `draft` (private checkpoint) → `published` (eligible to offer for
review) → `final` (canonical text). A snapshot is never edited; the working copy
moves on and the next snapshot captures it.

**Comment** — `open` → `resolved` (the manuscript author's call, not the
reviewer's) or `deleted` (soft, so threads keep their shape).

## Design notes

**Why `version_entries` duplicates chapter and scene titles.** A snapshot has to
be readable years after a chapter was renamed or deleted. Copying the titles in
makes the manifest self-contained, so a diff or a review reader never joins back
to live tables that may have moved on.

**Why scores are recomputed rather than incremented.** `comments.score` is a
cache of `SUM(comment_votes.value)`. Every vote recomputes it inside the same
transaction, so the cache cannot drift no matter how the vote sequence
interleaves.

**Why word counts are recomputed.** `works.word_count` is `SUM(scenes.word_count)`
recalculated on every scene write, delete and chapter removal — cheap at this
scale, and it cannot go wrong.
