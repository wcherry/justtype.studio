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

### 1.1 Editor durability · **Done**
- ✅ Offline buffer in IndexedDB, one entry per scene, replayed on reconnect and
  on load. Backoff retry as well as the `online` event, because a server that
  cannot be reached does not fire one.
- ✅ Conflict detection: `baseHash` on every save, refused with `409` and the
  state needed to resolve it. Both clients send it; the web offers keep-mine /
  use-theirs, iOS offers reload / keep-mine.
- ✅ "Restore this revision" in the UI, with a diff of what restoring would
  change. A restore writes a revision of the text it replaces, so it is undoable.
- ✅ Read cache of the outline and opened scenes, so the editor still opens with
  no network and the writer can reach their queued words.
- **Done when** a writer can lose the network mid-sentence, keep typing for ten
  minutes, and lose nothing on reconnect. *Verified in the browser: killed the
  backend mid-sentence, kept typing, reloaded the tab twice, restarted the
  server, and the text arrived with no user action.*

Three bugs this surfaced, all fixed: a network failure on `/auth/me` was
clearing the tokens and bouncing the writer to sign-in; reopening a scene showed
the stale server copy over unsynced queued work; and a blocked IndexedDB `open`
fires no event at all, so the editor hung forever waiting on a cache lookup.

### 1.2 Distraction-free writing · **Done**
- ✅ Focus mode: the paragraph the caret is in stays lit, the rest dim. A
  ProseMirror decoration, never a mark, so the author's document carries no
  record of where their cursor was.
- ✅ Typewriter scrolling: the caret holds a fixed height (45% — slightly above
  centre, where the eye rests) instead of walking to the bottom edge.
- ✅ Adjustable typeface, size, measure and line height, stored per device
  because the right measure depends on the screen in front of you.
- ✅ Session goals: words this sitting, words today against a daily target, a
  per-scene target, and a streak. Today and the streak are server-side, so they
  survive a reload and follow the writer between devices.
- **Done when** a writer can dim everything but the line they are on, keep the
  caret at a fixed height, tune the page to their eyes, and see today's words
  against a target — with the settings surviving a reload. *Verified in the
  browser: focus mode dims correctly, the caret held its anchor to within 6px
  after typing five paragraphs, and the goal bar went from "+0 this sitting" to
  "+12 this sitting · 244 of 200 today · Target met".*

Two bugs this surfaced, both fixed. The prose column was not a scroll container
at all — the window scrolled instead, so typewriter scrolling had nothing to
drive. And "this sitting" was held in a ref, which any remount reset, silently
restarting the count at whatever the total happened to be; the baseline now
lives in sessionStorage, which is scoped to the tab exactly like a sitting.

Deliberately not done here: none of this reaches iOS, whose editor is still
plain text. Focus mode and typewriter scrolling only mean something once *4.1*
gives it a real editor.

### 1.3 Manuscript formatting · **Done**
- ✅ Smart quotes, em dashes and ellipses as you type, narrowed to the
  punctuation a typesetter would fix anyway. The rules aimed at technical
  writing are off: `1/2` becoming ½ is actively wrong in fiction, where a
  character might say "half past".
- ✅ Scene-break glyphs (`#` in manuscript format, `⁂` in a book), chapter
  openings on a fresh page, and front matter — byline, short title, dedication,
  epigraph, copyright line — with contact details on the writer rather than the
  work, since they are the same for every submission.
- ✅ Export to DOCX, EPUB, PDF and plain text, in either standard manuscript
  format (Shunn) or a book layout. One assembled model feeds every writer, so
  the formats cannot disagree about what the book says — only about how it
  looks.
- **Done when** a writer can hand the file to an agent without reformatting it.
  *Verified by generating all four: the DOCX and EPUB unzip with every XML part
  well-formed, and macOS's own PDF engine renders the title page in Courier with
  the contact block, rounded word count and `Surname / TITLE / page` header
  exactly where Shunn puts them.*

The PDF writer is hand-rolled and Courier-only, which sounds like a limitation
and is really the same decision twice: Courier is a base-14 font, so it never
needs embedding, and it is monospaced, so line breaking is arithmetic rather
than font metrics — and it is exactly what standard manuscript format asks for.
A book-styled PDF would need proportional type, and therefore real metrics; that
is the one export shape still missing, and DOCX covers it in the meantime.

Two bugs this surfaced: a template's empty scenes exported as pages of lone `#`
marks, and the front-matter fields had nowhere to be set from — the work's PATCH
handler knew nothing about them, so a pen name saved silently did nothing.

### 1.4 Structure tools · **Done**
- ✅ Corkboard: the whole work as index cards, grouped by chapter, with drag to
  reorder and to move a scene into another chapter. Plain HTML5 drag and drop —
  one draggable element and one drop target is less code than a library for it.
- ✅ Scene metadata — POV, location, story date, status — surfaced as filters,
  offered as the values that actually exist rather than typed. `storyDate` is
  free text: fiction rarely runs on the Gregorian calendar.
- ✅ Split and merge, cutting between blocks rather than inside them. Lossless by
  construction, and guarded by `baseHash` on every scene involved.
- ✅ Story bible: characters, places and continuity notes, linked to scenes from
  both directions.
- **Done when** a writer can rearrange a novel without opening a single scene,
  and split one in two without losing a word. *Verified in the browser: a scene
  dragged from Act I to Act II, all four filters narrowing correctly, and a
  split then merged back with the text identical to the character.*

The property worth stating: `split` and `merge` are inverses. A unit test
asserts the round trip at every block boundary, and the smoke test asserts it
through the real API, because a bug here loses prose rather than merely
misordering it.

### 1.5 One source of truth for types · **Done**
- ✅ `docs/openapi.json` is generated from the handlers themselves: the axum
  routes and the document come from the same `#[utoipa::path]` annotations, so
  a handler cannot be routed without being documented.
- ✅ `cargo xtask codegen` emits `web/src/api/schema.ts` and the Swift
  `API` namespace from that document. One generator, two emitters, and it
  errors on any schema shape it does not understand rather than emitting `any`.
- ✅ `cargo xtask codegen --check` runs in CI and in `cargo xtask check`, so
  types that were not regenerated cannot merge.
- ✅ The closed string sets (work kind, scene status, visibility, comment
  category, …) are declared once in `domain::enums` and drive both the
  generated unions and the handlers' own validation.
- **Done when** a field renamed in Rust breaks the web and iOS builds.
  *Verified by renaming `Scene.word_count` to `words` on the wire — a change the
  Rust compiler is happy with — regenerating, and watching both client builds
  fail.*

One caveat worth knowing: the web errors name the field precisely, while
SwiftUI often reports a missing member inside a view body as "unable to
type-check this expression in reasonable time". The build fails either way, but
the iOS diagnostic can be misleading. Regenerate first when it appears.

---

## Phase 2 — Versioning and change tracking

### 2.1 Version ergonomics · **Done**
- ✅ Rename and annotate a snapshot after the fact. Its text is immutable; what
  you call it should not be, because "v3" is rarely the name that helps six
  months later.
- ✅ Tags, case-folded and idempotent, plus a singular pin for the one snapshot
  a writer wants to find without looking — usually what they last sent out.
- ✅ Restore into the working copy, never in place: the draft being replaced is
  snapshotted first, automatically, so a restore is itself undoable.
- ✅ Chapter- and scene-level snapshots, for checkpointing one chapter before
  rewriting it without implying anything about the rest.
- **Done when** a writer can find the version they sent out, put it back, and
  still have the draft it replaced. *Verified through the UI: renamed, tagged
  and pinned a version, restored it over a diverged draft, and found the
  replaced draft waiting on the shelf as "Before restoring …".*

The safety snapshot is the whole design. Restoring has to remove scenes written
after the version being restored — otherwise it is a merge, not a restore — and
removing a writer's work is only acceptable because it is one click from coming
back.

### 2.2 Better diffs · **Done**
- ✅ Paragraph-aligned, with moved-block detection. A scene diffed as one run of
  words is unreadable past a few sentences: changes smear across paragraph
  boundaries and every reflowed line looks edited.
- ✅ Inline and side-by-side modes.
- ✅ Diff a version against the working copy — `to=working`. "What have I
  changed since I last snapshotted" is the question writers ask most and it had
  no answer.
- ✅ A scene against any of its own revisions, in the same aligned shape.
- **Done when** a writer can see what changed without reading the whole scene.
  *Verified with a scene where one paragraph moved, one was edited, one was cut
  and one was added: all four read correctly, and the unchanged one is not
  shown at all.*

The ordering matters more than the algorithm. Exact matches are resolved before
any similarity pairing, because an identical paragraph elsewhere is a certainty
that it moved while similarity is only a guess. Getting that backwards — which
the first implementation did — let a guess consume a paragraph that had merely
moved, reporting a spurious edit *and* a spurious insertion, the worst of both
readings. There is a regression test named after it.

### 2.3 Tracked changes, applied · **Done**
- ✅ Accepting a change applies it — on the server, not in the editor. The
  original plan put this in ProseMirror on the reasoning that only it could map
  a position through later edits, but that made "accepted" mean something
  different on every client, ruled out phones, and made a batch twenty round
  trips. The mapping turns out not to need a transform history.
- ✅ Position mapping: a proposal records the `contentHash` it was written
  against. An unchanged scene applies exactly; a changed one re-anchors by its
  quotation; anything ambiguous is refused as stale and left pending, with a
  reason, rather than guessed at.
- ✅ Accept-all / reject-all per scene, optionally filtered to one author.
  Batches apply back to front, so applying one never disturbs the positions of
  the ones still to come.
- ✅ Suggesting from a selection, with the replacement and a note.
- **Done when** an author can accept twenty suggestions in a row and the
  document is exactly what the reviewer intended. *Verified: twenty proposals
  renumbering twenty words, accepted in one call, producing the intended text
  exactly — and a proposal whose text had since vanished left pending with
  "`vanished` is no longer in this scene".*

Two deliberate limits. A change spanning more than one paragraph is refused:
joining blocks means deciding which one's type and attributes survive, and such
a selection is nearly always an accident rather than an intention. And full
suggestion mode — where a collaborator's *typing* becomes proposals — waits on
there being collaborators at all (*5.3*); today only a work's owner can reach
its scenes, so the selection-based flow is the whole of what the feature can
currently mean.

### 2.4 Comparison across publications
- **Done:** "what changed since reviewers last saw this" as a first-class view.
  A diff answers "what did I change"; this answers the question an author
  actually has in front of a pile of feedback — *which of these have I already
  dealt with*. Each note is placed against the paragraph it was anchored to and
  that paragraph is followed forward, so every note comes back as still
  standing, written since, on text that has gone, or general. The ones already
  written can be resolved in one action.
- **Still open:** notify prior reviewers when a new version answers their
  notes. This needs a notification system, which does not exist — email means
  a sending domain and credentials, and the in-app alternative is an inbox
  nobody has asked for yet. Left undone deliberately rather than half-built:
  the author-side view above is the half that works without one.

---

## Phase 3 — Publication and review

### 3.1 Review workflows — **done**
- Review windows with an open and close date, and a reviewer cap. Outside the
  window every reader is refused; the author can always open their own.
- `invited` visibility: named readers by email address, singly or by circle.
  Addresses rather than accounts, because the people a writer trusts with a
  draft usually have an address and not yet an account here.
- Private beta-reader circles, so inviting "my first readers" is one action
  rather than eleven.
- Reader progress, recorded as the reader scrolls and reported to the author as
  a percentage. It keeps the furthest point reached, not the latest: giving up
  in chapter three and later skimming the ending still means chapter three.
- The feed answers the same question the click does — an invited reader sees
  what they were invited to, and nobody is shown a window that has shut. There
  is no mail going out yet (*2.4*), so the feed is the only way in.

### 3.2 Richer feedback — **done**
- One-tap reactions — loved / confusing / dragging — in the margin beside every
  paragraph. Anchored to the block rather than to a selection, because two
  readers never highlight quite the same words and a tally nobody can add up is
  not a signal. The author gets the totals; a reader sees only their own marks,
  so the fifth person to call a paragraph confusing still means it.
- The author's own questions, rated 1–5 or answered in words, asked once or
  asked again at the end of every chapter — which is what turns "does the
  middle sag" into something with an answer. They are asked where the chapter
  ends, while the reader still has it in mind.
- Reviewer standing: votes plus the notes an author marked resolved, weighted
  so that an author acting on a note outweighs a bystander agreeing with it.
  Derived on read rather than stored, and shown beside the name on every note.

### 3.3 Comment management — **done**
- Filters by scene, category, score, resolved state and reviewer. They apply to
  the thread rather than the sentence: hiding a reply would leave an answer
  without its question.
- Bulk resolve, because twenty typo reports are twenty rows and one decision. A
  bulk call with nothing to narrow it is refused rather than resolving a whole
  review on one mis-click.
- Revision tasks — the step between forty notes and a plan. A note the author
  accepts becomes a line of work against the scene, on a list in the editor,
  and it outlives the publication it came from because the next draft is where
  it gets answered. Not a tracked change: "chapter four needs to be colder" is
  not a diff.
- The whole review as one document, through the same writers the book uses, so
  it can be printed and marked up rather than scrolled past.

### 3.4 Moderation and safety — **done**
- Report a comment. There is no moderation team, so it goes to the only person
  with standing to act on it: the author whose work it sits on. Nothing is
  hidden automatically — a report is one reader telling the author to look.
- An author-side block list: one writer's door, not a platform ban. Enforced in
  `publication_for_reader`, so it covers everything a reader could do rather
  than the handlers somebody remembered to guard.
- Rate limits on comments, votes and reactions, per user per minute, counted in
  process. They exist to stop a script, not a person, and the refusal says how
  long to wait.
- Spoiler-safe serials: notes on scenes a reader has not reached are withheld,
  using the same furthest-point index reader progress already keeps. On by
  default for a serial, where not knowing yet is the point of reading one.

---

## Phase 4 — iOS parity

### 4.1 Real rich-text editing — **done**
- The mobile editor is the same TipTap the browser runs, hosted in a
  `WKWebView` and shipped inside the app: one editor, one document model and
  one set of ProseMirror positions across all three clients. Chosen with a
  spike rather than an argument — `spikes/4.1-mobile-editing/README.md` has the
  measurements, including what an attributed-string model loses.
- The read-only guard is gone, and so is `hasRichFormatting`: there is no
  longer a document the phone refuses to open. A format bar reaches the six
  things a thumb needs, and the marks a selection carries light up in it.
- A scene saved through the web client and read back from the server decodes,
  re-encodes and compares byte-for-byte in `RichSceneTests` — link attributes
  included, which are the easiest thing in a document to drop.
- Five UI tests drive the real editor on a simulator: it loads and renders,
  typing reaches Swift, a selection reports itself, the toolbar's bold reaches
  the document Swift holds, and ⌘Z puts the words back.
- The keyboard question from the spike is **answered**: with the editor as it
  now stands the software keyboard presents (233 points tall, top at y=583 on
  an 874-point screen) and the line being edited sits at y=207, well clear of
  it. The test keeps its skip for the case where the keyboard is parked
  off-screen, because a measurement against a keyboard that is not there is
  not a measurement.
- Two defects the simulator found and fixed: loading a scene left the selection
  at the end of the document, so the browser scrolled to it and hid the first
  paragraph; and the load itself sat on the undo stack, so one undo emptied the
  scene and autosave would have persisted that. Loading is not an edit.
- The format bar carries undo and redo, because a phone has no ⌘Z and shaking
  it at a paragraph is a gamble rather than a design.

### 4.2 Offline-first — **done**
- `LocalStore`: the works, the outlines and every scene the writer has opened,
  on the phone as files, written the way the server writes manuscripts —
  neighbour then rename, because a half-written scene is worse than no scene.
- `SaveQueue`: one pending save per scene, on disk, surviving the app being
  killed. A sitting of edits collapses to the latest text measured against the
  hash the *first* unsent edit was made on — the ones after it were written
  over text the server has never seen.
- `SyncEngine`: reads prefer the server and fall back to the phone; writes go
  to the phone first and to the server if they can. A queued edit always beats
  the server's copy on reopening, because handing a writer their own work back
  undone is the worst thing an offline mode can do.
- The same conflict rules as *1.1*: a queued save onto a scene that moved is
  refused, kept, and stops being retried until the writer chooses. "Keep mine"
  rebases onto whatever the server has now.
- Seven tests fly the plane: the network is a stub that switches off mid-edit,
  so the client takes its usual path and gets the failure iOS hands it in a
  tunnel. Queued, reopened, collapsed, flushed, conflicted, rebased, and
  surviving a crash.
- The state is shown rather than hidden — "Offline. 2 scenes waiting to go up"
  — and the queue flushes when the app comes back to the foreground.

### 4.3 Mobile review — **mostly done**
- Character-level anchoring, which came almost free once the reader became the
  same ProseMirror document the editor is: one flag turns the caret off and
  leaves the text selectable, and the `{from, to}` a long press produces is the
  pair the web client would send for the same words. A UI test presses a word
  and checks what comes back is a word and a real range, not a block.
- Reader mode: size, face and leading, kept between sittings, and paging —
  columns the width of the screen with snapping, because a novel in portrait is
  a stack of pages rather than a scroll.
- Reading on a phone now counts: the reader records progress as it goes, so the
  author's "how far did they get" is not a browser-only signal.
- **Not built: push notifications.** They need APNs credentials, a signed push
  entitlement and a service to send them — the same missing piece as *2.4*'s
  half. Nothing here can be tested or even meaningfully written without one, so
  it is left undone rather than half-built. It belongs with the notification
  work, not with the reader.

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

### 6.1 Accounts
- OAuth sign-in (Apple, Google) alongside the existing password flow.
- Email verification and password reset.
- Two-factor authentication; session list with remote revoke.

### 6.2 Trust
- GDPR export and delete, covering the filesystem as well as the database.
- Audit log of publications, permission changes and deletions.
- Terms that make it unambiguous the author keeps their copyright.

### 6.3 Operations
- Structured request logging with a request id through to the client.
- Error tracking, uptime checks, and a status page.
- Nightly backups of both the database and the manuscript tree, with a
  restore drill that is actually run.

### 6.4 Plans and billing
- Stripe subscriptions; free tier capped on works, snapshot retention and
  concurrent review windows.
- Usage metering on storage and snapshot count.
- Dunning, proration, and a self-serve billing portal.

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
