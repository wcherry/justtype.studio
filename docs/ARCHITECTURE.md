# Architecture

## The three decisions that shape everything else

### 1. Manuscript content lives on disk; everything else lives in SQLite

Scene bodies are TipTap JSON documents. They are large, they are written
constantly, they are read whole, and they are never queried by their contents.
That is a file, not a row. SQLite holds the metadata and the relationships:
who owns what, what order it is in, what has changed, who said what about it.

```
data/works/<work_id>/
  working/<scene_id>.json                  mutable draft surface
  revisions/<scene_id>/<revision_id>.json  autosave history
  versions/<version_id>/version.json       snapshot manifest
  versions/<version_id>/scenes/<id>.json   frozen snapshot bodies
```

Every write is write-then-rename, so a crash mid-save cannot leave a half-written
scene. Every path is built from an id that has been validated against a strict
character set, so a request cannot walk out of the data directory.

All of this sits behind `storage::FileStore`. Moving to object storage later
means reimplementing that one type.

### 2. Comments anchor to immutable snapshots, not to the working copy

A comment carries a `{from, to}` pair of ProseMirror positions plus the quoted
text. Those positions are meaningful only against a specific document. If they
pointed at the live draft, every keystroke the author typed would slide them out
of alignment, and the usual answer — position mapping through the edit history —
is a large amount of machinery to get approximately right.

Snapshots never change. So an anchor into a snapshot is exact, permanently, for
free. This is why `publish` exists as a step between "I wrote something" and
"read this": publishing is what creates the stable thing reviewers point at.

The cost is that feedback on version 3 does not automatically appear on version
4. That is honest — it is feedback on version 3 — and the comparison view built
in *2.4* turns it into a feature rather than a gap: because the anchor is exact
against a snapshot that cannot move, the paragraph under each note can be
followed forward into the newer text, and the author is told which notes they
have already written past.

### 3. Positions are computed identically on all three clients

The web editor reports `editor.state.selection.from`. The Swift client computes
the same number from the raw JSON in `DocNode.blocks()`. Both count text in
UTF-16 code units, because that is what ProseMirror counts. `DocNodeTests` locks
this down, emoji included. A comment anchored on an iPhone highlights exactly
the right words in a browser.

## Backend

```
main.rs        boot: config, pool, migrations, template seeding, router
config.rs      environment parsing, fails fast on a weak JWT secret
error.rs       ApiError -> HTTP status + JSON envelope
state.rs       AppState: config, pool, file store
auth/          Argon2 hashing, JWT issue/verify, AuthUser extractor
db/            r2d2 pool with per-connection pragmas, schema, row models
domain/        document flattening, diffing, built-in templates
storage/       the file store
routes/        one module per resource, plus `support` for access checks
```

**Diesel is synchronous**, so every query runs through `db::blocking`, which
hops onto Tokio's blocking pool and hands back an `ApiResult`. Handlers never
touch the connection pool directly.

**Authorisation lives in one file.** `routes/support.rs` holds
`work_for_owner`, `scene_for_owner`, `publication_for_reader` and friends. Every
handler that touches a work goes through one of them, so the rules are stated
once rather than re-derived in twenty places.

**SQLite is configured per connection**: WAL, foreign keys on, a busy timeout.
A test asserts foreign keys are actually enabled, because a silently ignored
`PRAGMA` is the kind of thing that only shows up as orphaned rows months later.

**Migrations are tested in both directions.** A `down.sql` is written once and
never run, so it rots silently. The test peels the stack one migration deeper
each time and re-applies it, rather than reverting everything at once — the
first migration drops every table, so a full rollback would hide a later
`down.sql` that failed to remove its own column, because the table it lived on
is dropped regardless. Peeling progressively means each migration's `down.sql`
is followed by its own `up.sql`, and one that did not undo its migration fails
to re-apply with a duplicate column.

`backend/build.rs` exists for the same reason: `embed_migrations!` bakes the SQL
in at compile time, and without a `rerun-if-changed` cargo does not notice a
migration changing. Editing one and running the tests would recompile nothing,
pass against the old SQL, and look fine until it ran somewhere real.

## Web

Vite + React + TipTap. The interesting part is `editor/commentAnchors.ts`:
comment highlights are ProseMirror **decorations**, not marks. A mark would mean
a reviewer's highlight becomes part of the author's document. A decoration is a
view-layer overlay that touches nothing.

Autosave is two-tier (`editor/useAutosave.ts`): a 1.5-second debounce keeps the
server copy current, and a slower pass also asks the server to keep a revision.
Both flush on tab hide and before unload.

## iOS

A Swift package with two targets — `ScriptoriumKit` (models, API client,
document maths) and `ScriptoriumUI` (SwiftUI views) — plus a thin app target.
The package builds for macOS as well as iOS, so `swift build` and `swift test`
run in CI without a simulator.

The mobile editor edits plain paragraphs. A scene that carries marks or
non-paragraph blocks opens **read-only** rather than flattening the author's
formatting on save. Losing someone's italics silently is worse than telling them
to open it on the web. *Phase 4.1* removes the limitation properly.

## What this architecture does not do

It runs as one process. SQLite serialises writes, and the manuscript tree is a
local filesystem.

At the load this product expects — on the order of **3–4 transactions per
second** — that is not a constraint worth engineering around. A single SQLite
file in WAL mode absorbs that without noticing, and the workload suits it:
manuscript bodies are already files, and the rows beside them are small,
per-author, and read far more often than written. The work that actually matters
here is replication and restore, not throughput. *Phase 7.1* covers it.

If real-world numbers ever contradict that, the fallback is Postgres behind
Diesel: the schema is portable as written and every query already goes through
`db::blocking`. It is a contingency to keep cheap, not a design to build toward.

The manuscript tree is the boundary more likely to move first, and it is already
abstracted behind `FileStore`.
