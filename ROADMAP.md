# Scriptorium — Feature Roadmap

A writing tool for long-form fiction, and a review surface for the readers the
writer chooses. Authors draft in chapters and scenes, snapshot versions as they
go, publish a version for review, and get anchored, votable feedback back.

Status legend: **Done** — built and verified in this repo · **Next** — the
immediate slice · **Later** — sequenced but not started.

---

## Phase 0 — Skeleton · **Done**

Everything below is implemented, compiles, and is covered by the smoke test
in `docs/` or by unit tests.

| Feature | Where |
|---|---|
| Rust backend: Axum 0.8 + Diesel 2.2 + SQLite (WAL, foreign keys on) | `backend/src` |
| File-backed manuscript storage, atomic write-then-rename | `backend/src/storage/mod.rs` |
| Email + password auth: Argon2id hashes, JWT access tokens, rotating refresh tokens stored as digests | `backend/src/auth` |
| Works, chapters, scenes: CRUD, reordering, move-between-chapters | `backend/src/routes/{works,scenes}.rs` |
| Five built-in structural templates, re-seeded idempotently on boot | `backend/src/domain/templates.rs` |
| Scene autosave with revision history | `backend/src/routes/scenes.rs` |
| Immutable version snapshots with a flattened manifest | `backend/src/routes/versions.rs` |
| Version lifecycle: draft → published → final | `backend/src/routes/versions.rs` |
| Word-level diff between any two versions | `backend/src/domain/diff.rs` |
| Publications: visibility, scope, review prompt | `backend/src/routes/publications.rs` |
| Comments: anchored, threaded, categorised, votable, resolvable | `backend/src/routes/comments.rs` |
| Tracked changes: propose / accept / reject | `backend/src/routes/changes.rs` |
| React web client: TipTap editor, outline, versions panel, diff view, review reader, comment sidebar | `web/src` |
| Highlight-to-comment via ProseMirror decorations (never marks) | `web/src/editor/commentAnchors.ts` |
| SwiftUI iOS client: library, outline, plain-text scene editor, versions, feed, paragraph-anchored review | `ios/ScriptoriumKit/Sources/ScriptoriumUI` |
| Shared position arithmetic, unit-tested to match ProseMirror | `ios/ScriptoriumKit/Sources/ScriptoriumKit/DocNode.swift` |

### Known gaps carried out of Phase 0

These are deliberate, not oversights. Each has a home below.

- **iOS edits plain text only.** A scene containing marks or non-paragraph
  blocks opens read-only rather than silently flattening the author's
  formatting. → *Phase 4.1*
- **One backend process.** SQLite serialises writes and the manuscript tree is a
  local filesystem. At the expected 3–4 transactions per second this costs
  nothing; what it does need is replication and backups. → *Phase 7.1*
- **No collaboration.** One author per work; no co-writers, no live editing.
  → *Phase 5.3*, *Phase 7.3*
- **Tracked changes are recorded, not applied.** The server stores the proposal
  and its resolution; the editor applies an accepted change. → *Phase 2.3*
- **Types are hand-mirrored** across Rust, TypeScript and Swift. → *Phase 1.5*

---

## Phase 1 — Writing that holds up

The editor has to be good enough that a novelist stops using Scrivener. That is
the bar for this phase.

### 1.1 Editor durability
- Offline buffer in IndexedDB; replay queued saves on reconnect.
- Conflict detection: send the scene's `contentHash` with every save and reject
  a write whose base has moved, instead of overwriting another tab's work.
- "Restore this revision" in the UI, wired to the existing revision endpoints.
- **Done when** a writer can lose the network mid-sentence, keep typing for ten
  minutes, and lose nothing on reconnect.

### 1.2 Distraction-free writing
- Focus mode: current paragraph lit, the rest dimmed.
- Typewriter scrolling; adjustable measure, line height and font.
- Session goals: words this session, streaks, a target per scene or per day.

### 1.3 Manuscript formatting
- Smart quotes, em-dashes, ellipses as you type.
- Scene-break glyphs, chapter title pages, front matter.
- Export to DOCX, EPUB, PDF and standard manuscript format (Shunn).

### 1.4 Structure tools
- Corkboard / index-card view over scenes; drag to reorder across chapters.
- Scene metadata surfaced as filters: POV, location, timeline date, status.
- Split and merge scenes with positions preserved.
- Story bible: characters, places, and continuity notes linked from scenes.

### 1.5 One source of truth for types
- Generate an OpenAPI document from the Axum routes (`utoipa`).
- Generate `web/src/api/types.ts` and the Swift models from it in CI.
- **Done when** a field renamed in Rust breaks the web and iOS builds.

---

## Phase 2 — Versioning and change tracking

### 2.1 Version ergonomics
- Name, annotate and tag snapshots; pin a "last submitted" version.
- Restore a version into the working copy as a new draft, never in place.
- Chapter- and scene-level snapshots, not just whole-work.

### 2.2 Better diffs
- Paragraph-aligned diffs with moved-block detection, not just word runs.
- Side-by-side and inline diff modes; diff a version against the working copy.
- Diff a scene against any of its own revisions.

### 2.3 Tracked changes, applied
- Editor-side application of an accepted change, with position mapping through
  edits made since the proposal.
- Suggestion mode: a collaborator's typing becomes proposals instead of edits.
- Accept-all / reject-all per scene, per chapter, per author.
- **Done when** an author can accept twenty suggestions in a row and the
  document is exactly what the reviewer intended.

### 2.4 Comparison across publications
- "What changed since reviewers last saw this" as a first-class view.
- Notify prior reviewers when a new version answers their notes.

---

## Phase 3 — Publication and review

### 3.1 Review workflows
- Review windows with a close date and a reviewer cap.
- Invite specific readers by email or handle; private beta-reader circles.
- Reader progress: how far through, where they stopped.

### 3.2 Richer feedback
- Inline reactions (confusing / dragging / loved this) as one-tap signals.
- Per-chapter rating prompts driven by the author's review questions.
- Reviewer reputation from vote totals, weighted by the author's own resolves.

### 3.3 Comment management
- Filter by scene, category, score, resolved state, author.
- Bulk resolve; convert a comment into a revision task on the author's list.
- Export all feedback as a working document.

### 3.4 Moderation and safety
- Report a comment; author-side block list.
- Rate limits on comment creation and voting.
- Spoiler-safe defaults for serial fiction.

---

## Phase 4 — iOS parity

### 4.1 Real rich-text editing
- `UITextView` bridge with a ProseMirror-compatible attributed-string model, or
  an embedded TipTap instance in a `WKWebView` — decide with a spike.
- Until then the current read-only guard stays; it is the correct failure mode.
- **Done when** a scene written on the web opens, edits and saves on iPhone
  without losing a single mark.

### 4.2 Offline-first
- Local mirror of the outline and the scenes the writer has opened.
- Background sync with the same conflict rules as *1.1*.
- Writing on a plane is the acceptance test.

### 4.3 Mobile review
- Character-level selection for anchoring, replacing paragraph-level tapping.
- Reader mode with pagination and adjustable typography.
- Push notifications for new comments and replies.

### 4.4 Platform polish
- Widgets for the daily word goal; Shortcuts for "start writing".
- iPad: split view with the outline, hardware keyboard shortcuts.
- Dictation capture straight into a scene.

---

## Phase 5 — Community and discovery

### 5.1 Profiles and following
- Author profiles: bio, published works, review history.
- Follow authors; a feed of new publications from people you follow.

### 5.2 Discovery
- Browse by form, genre, length and review-window state.
- "Needs readers" queue that surfaces under-reviewed work first.
- Reciprocity: reviewing others' work raises your own placement.

### 5.3 Collaboration
- Co-authors on a work, with roles (owner, writer, editor, reader).
- Per-chapter permissions for ghostwriting and editorial hand-offs.
- Activity feed per work.

---

## Phase 6 — The SaaS parts

### 6.1 Plans and billing
- Stripe subscriptions; free tier capped on works, snapshot retention and
  concurrent review windows.
- Usage metering on storage and snapshot count.
- Dunning, proration, and a self-serve billing portal.

### 6.2 Accounts
- OAuth sign-in (Apple, Google) alongside the existing password flow.
- Email verification and password reset.
- Two-factor authentication; session list with remote revoke.

### 6.3 Trust
- GDPR export and delete, covering the filesystem as well as the database.
- Audit log of publications, permission changes and deletions.
- Terms that make it unambiguous the author keeps their copyright.

### 6.4 Operations
- Structured request logging with a request id through to the client.
- Error tracking, uptime checks, and a status page.
- Nightly backups of both the database and the manuscript tree, with a
  restore drill that is actually run.

---

## Phase 7 — Scale and hardening

### 7.1 Storage
SQLite is the database. Expected load is **3–4 transactions per second**, which
is two to three orders of magnitude below what one SQLite file in WAL mode
handles comfortably. There is no scaling problem here to pre-solve, and this
phase should stay small.

- **Durability, not throughput, is the real work.** Streaming replication with
  Litestream: continuous backup to object storage, point-in-time restore, and a
  restore drill that is actually run.
- **Instrument the write path** so the assumption above is checked rather than
  believed: transactions per second, write latency, and lock-wait time on a
  dashboard.
- **Object storage for manuscripts** (S3-compatible) behind the existing
  `FileStore` interface, if the tree outgrows one disk. Independent of the
  database, and the likelier of the two to be needed.

**If the numbers ever say otherwise**, the answer is Postgres behind Diesel. The
schema is portable as written and every query already runs through
`db::blocking`, so it is a contained change. It is a contingency, not a plan —
do not build toward it, and do not carry a compatibility layer for it.

### 7.2 Performance
- Cache assembled version content; snapshots are immutable, so caching is free.
- Paginate the review reader for novel-length works.
- Full-text search across a work, and across the review feed.

### 7.3 Real-time
- WebSocket channel for live comment updates in the review reader.
- CRDT-backed co-editing (Yjs + `y-prosemirror`) for co-authored works.
- Presence indicators.

### 7.4 Security
- Argon2 parameter tuning and a rehash-on-login path.
- Per-route rate limiting; brute-force protection on sign-in.
- Signed, expiring URLs for unlisted publications.
- Independent security review before the first paid customer.

---

## Sequencing note

Phases 1–3 are the product. Phase 4 is the second client. Phases 5–7 are the
business around it. If the roadmap has to be cut, cut from the bottom: a writing
tool that people trust with a novel earns its way to the rest.
