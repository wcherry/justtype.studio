# Just Type Studio — Feature Roadmap

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
| SwiftUI iOS client: library, outline, plain-text scene editor, versions, feed, paragraph-anchored review | `ios/JustTypeKit/Sources/JustTypeUI` |
| Shared position arithmetic, unit-tested to match ProseMirror | `ios/JustTypeKit/Sources/JustTypeKit/DocNode.swift` |

### Known gaps carried out of Phase 0

These are deliberate, not oversights. Each has a home below.

- **iOS edits plain text only.** A scene containing marks or non-paragraph
  blocks opens read-only rather than silently flattening the author's
  formatting. → *Phase 4.1*
- **One backend process.** SQLite serialises writes and the manuscript tree is a
  local filesystem. At the expected 3–4 transactions per second this costs
  nothing; what it does need is replication and backups. → *Phase 7.1*
- **No collaboration.** One author per work; no co-writers, no live editing.
  → *Phase 5.3* (co-authors, done), *Phase 7.3* (live editing)
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

### 4.4 Platform polish — **done**
- A home-screen widget for today's words against the goal, with the streak. It
  reads a small file the app leaves in a shared app group rather than fetching
  anything: a widget cannot sign in, and giving an extension a refresh token so
  it can draw a progress bar would be a credential in a second place for no
  reason. The app rewrites the file and reloads the timeline whenever the
  number moves.
- A Shortcut and Siri phrase for "start writing", which opens the scene the
  writer had last. An intent runs before the interface exists, so it leaves a
  note and the app picks it up.
- iPad: `NavigationSplitView`, so the outline sits beside the page — it costs
  nothing on a phone, where it collapses to exactly the stack it was.
  ⌘⌥↑/↓ walk the scenes, ⌘B/⌘I/⌘U format, ⌘S checkpoints.
- Dictation is the system keyboard's. The editor is a standard editable
  region, so the microphone key types into it like any other field; a separate
  speech-recognition path would duplicate a platform feature and could not be
  tested here.
- **Not verified on a running iPad:** the split view itself. Reaching it needs
  a signed-in session, and setting one up means typing a password into the
  simulator. The editor and reader were both exercised on an iPad — which is
  where the one real iPad bug turned up.

---

## Phase 5 — Community and discovery

### 5.1 Profiles and following — **done**
- Author profiles: a bio, what the author has on offer, and their record as a
  reviewer. What is on offer is the feed with an author filled in, not a second
  query, so a profile lists exactly what the viewer could open — never a
  locked door, never an invited draft. The review history is limited to work
  the viewer could see too: a note on a privately shared draft, listed on a
  profile, would tell strangers the draft exists.
- Follow an author, and a "Following" view of the feed with only their work.
  No approval step, because a follow buys nothing the author did not already
  choose to show. An author who has blocked you cannot be followed — and the
  feed now hides a blocking author's work altogether, which it did not before:
  the click was refused, but the card was still there inviting it.
- Names on cards, notes and the reader header lead to the profile.

### 5.2 Discovery — **done**
- Browse by form, genre, length and review window. Genre is a new, optional
  field on the work — a writer who does not know yet is not made to guess.
  Length goes by the published version's word count in the bands the awards
  use; the window by whether it closes within three days or never does. An
  unknown value is refused rather than matching nothing, so a typo cannot pass
  for an empty feed.
- A "needs readers" queue: least-read first, where "read" means someone
  opened it or left a note, and leaving out the caller's own work and anything
  they have already read — those have had the reader this caller could give.
- Reciprocity: each piece of someone else's work an author has left notes on
  lifts their own by one place, capped at ten so it nudges rather than buys.
  A publication counts once however many notes went on it; twenty typo reports
  on one chapter are one act of reading. The card says how much the author has
  read for others, so the queue's order is legible rather than mysterious.
  Both numbers are derived on read, like reviewer standing.

### 5.3 Collaboration — **done**
- Co-authors on a work, addressed by email like invited readers, with four
  roles: a reader reads the working draft, an editor proposes changes and keeps
  the to-do list, a writer changes the text and structure, an owner does
  everything else. The person who started the work is not a row on the list,
  so nothing on it can remove them, and only they can delete it.
- Access is one type, `support::Access`, and every handler that touches a work
  now asks it for an ability rather than asking "is this the owner". The old
  owner-only lookups for scenes and chapters were removed rather than kept as
  aliases, so the compiler walked every call site and each one says what it
  needs.
- Per-chapter grants for a ghostwriter or an editorial hand-off. They narrow
  what someone may change, never what they may read — chapter nine is written
  against the eight before it. Changing the book's shape needs the whole work.
  A grant is a flag plus a list, so deleting the last chapter on it leaves the
  collaborator nowhere, not everywhere.
- An activity feed per work, written by the handlers as they act rather than
  derived, because a derived feed cannot say who removed a chapter. A sitting
  of autosaves is one line, not two hundred. Collaborators are named there,
  never shown by address — only owners see addresses.
- The clients follow the server: the outline carries the caller's role, the
  web editor drops the caret and the format bar where the text is not theirs
  (an editor can still select words to propose a change), and the phone opens
  those scenes read-only rather than queueing saves that could never land.
- Co-authors are not readers: opening their own book does not count towards
  `readerCount`, and notes on it do not count as reading for others.
- **Not built: live co-editing.** Two writers on one scene still meet the
  1.1 conflict rules. Presence and CRDTs are *7.3*.

---

## Phase 6 — The SaaS parts

### 6.1 Accounts — **mostly done**
- Mail, behind one `Mailer`: an outbox file in development and in the smoke
  walkthrough, where a link is read back and followed exactly as from an inbox,
  and SMTP configured from the environment. The SMTP path compiles and is
  configured; it has not been run against a real relay, which needs a sending
  domain.
- Email verification, and the reason it matters: reader invitations and seats
  on a book are both addressed to an email, and until now anyone could register
  someone else's address and collect what was sent to it. An unproved account
  now sees nothing addressed to it.
- Password reset by mailed link: an hour, once, the newest only, and using it
  signs out every device. The same answer whether or not the address has an
  account, and limited per address so it is not a way to fill someone's inbox.
- Two-factor sign-in (TOTP, pinned to the RFC 6238 vectors), recovery codes,
  each code good once, tries limited. The key only counts once a code from it
  checks out, so a typo in setup cannot lock anyone out.
- Sessions: one per device, named in every access token and checked on every
  request, so signing a device out takes effect now rather than in half an
  hour. A list with "this one" marked, sign out one, sign out everywhere else.
- The iOS app asks for the code and for agreement to the terms.
- **Not built: OAuth** (Apple, Google) — it needs registered apps and keys, and
  was deferred rather than written blind.

### 6.2 Trust — **done**
- Export: one zip of everything held about a person — the records as JSON and
  every manuscript file exactly as stored — with no password hash in it.
- Delete: the account, the works it started and their files. A work with
  co-authors is refused by name until the person says to go ahead. What they
  made inside other people's books is not cascaded away: snapshots and
  revisions there go to that book's owner, notes are blanked and credited to a
  "former member" so the replies under them still read.
- That last part found a bug as old as comments: a deleted note took every
  reply under it out of the thread, which soft-deletion existed to prevent. A
  removed note someone answered now stays as an empty placeholder.
- An audit log of publications, invitations, blocks, co-authors, deletions and
  account security, readable by the people it concerns. No foreign keys: the
  line saying an account was deleted has to outlive the account.
- Terms (`docs/TERMS.md`) that say first that the writer keeps their copyright,
  agreed at sign-up, versioned so a change asks again. Written in plain
  language for the product; **they need a lawyer's review** before anyone pays.

### 6.3 Operations — **the in-repo half done**
- A request id on every response, every log line and every error envelope.
  The web client sends its own, so the id the browser saw is the id in the
  log, and a server failure shows the writer a reference to quote. JSON logs
  on request, one `served`/`failed` line per request with its timing, and a
  panicking handler answers in the usual envelope instead of dropping the
  connection.
- `/api/status` checks what actually fails — the database and its migrations,
  whether a save could land, whether last night's backup exists *and was
  restored* — and answers 503 when any does. `/status` shows it to a person.
- Backups (`cargo backup`): a consistent copy of the database taken while the
  server runs, the manuscript tree, and a manifest with a digest of every
  file. Each run then restores what it took somewhere disposable and checks it
  would serve — integrity, migrations, counts, digests, and that every draft
  and every snapshot scene has its file. A unit test and the smoke walkthrough
  both delete one file from a backup and watch the drill fail. Cron and launchd
  examples are in `docs/OPERATIONS.md`; nothing is installed on its own.
- **Not built: hosted error tracking, external uptime checks, a hosted status
  page.** They need providers. The endpoint to point them at exists.

### 6.4 Plans and billing — **deferred**
Not started, by decision: it needs Stripe keys and pricing that has not been
chosen. What it would cover:
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
  Litestream — **configured, not run.** `deploy/litestream.yml` and the
  replicate and point-in-time restore procedure are in `docs/OPERATIONS.md`;
  there is no bucket to run it against, so the first restore from a real one
  is the check that it works. The nightly backup and restore drill from *6.3*
  remain what keeps the manuscript files, which Litestream does not cover.
- **The write path is instrumented — done.** Write requests and database calls
  per second, write latency (pool wait included), and lock wait. SQLite does
  its lock waiting inside a busy handler Diesel does not expose, so lock wait
  is *sampled*: every five seconds a probe takes the write lock with
  `BEGIN IMMEDIATE` and times it — the wait a real write would have had. The
  last minute and fifteen are on `/status`, the status check fails at a p95
  lock wait of 250 ms, and `/api/metrics` serves Prometheus text behind a
  token for a real dashboard. On the smoke walkthrough: under a millisecond to
  write, a few hundredths of one waiting for the lock.
- **Object storage for manuscripts** (S3-compatible) behind the existing
  `FileStore` interface, if the tree outgrows one disk — **deferred**, as this
  section says it should be until a disk is actually filling.

**If the numbers ever say otherwise**, the answer is Postgres behind Diesel. The
schema is portable as written and every query already runs through
`db::blocking`, so it is a contained change. It is a contingency, not a plan —
do not build toward it, and do not carry a compatibility layer for it.

### 7.2 Performance — **done**
- Snapshot scene bodies are cached in memory, keyed by version and scene,
  least recently read evicted first under a byte budget. Only the bodies: a
  version's own row (label, state) does change, so the assembled response is
  never cached. Hits and misses are on `/api/metrics`.
- The review reader goes a chapter at a time (`?chapter=N`, with a table of
  contents), and a page reads only its own chapter's files. Progress is still
  counted along the whole book — the table of contents carries every scene in
  order, so moving to chapter two does not restart the count. Without
  `?chapter` the endpoint answers as before, which is how the iOS reader asks.
- Full-text search (SQLite FTS5) within a work's working text — a Find panel
  in the editor — and across the review feed, titles and published text both.
  The indexes are refreshed by the search that reads them, from what each row
  was built from, so they cannot drift from a write path that forgot them.
  Separate indexes for the short fields and the text, because FTS5 scores
  against a whole row's length and a novel's text would bury its own title.

### 7.3 Real-time — **live notices done; co-editing deferred**
- A WebSocket channel, opened with a single-use thirty-second ticket because a
  browser cannot send an Authorization header on a WebSocket and a token in a
  URL is a token in a log. It carries notices, never content: "the notes
  changed", "Kit saved Midpoint". The client fetches through the ordinary API,
  so spoilers, filters and blocks still decide what it sees, and a notice
  cannot leak what a fetch would refuse.
- Live comments in the review reader: a note someone writes appears for
  everyone reading, and so do resolves, deletions and votes.
- Presence: co-authors see each other by name and by scene in the editor;
  readers of a review see only how many others are reading. And when someone
  else saves the scene open in front of you, it says so at once — with a
  button to load their version — instead of at your next save's conflict.
- **Deferred: CRDT co-editing** (Yjs + `y-prosemirror`). It replaces the save
  model — whole-document saves with hash conflicts, the offline queues on web
  and iOS — for any scene two people have open, which makes it a rewrite of
  the write path rather than an addition, and it wants its own phase. The
  save notice above is what narrows the gap until then.
- iOS does not join the live channel yet; it keeps the 1.1 conflict rules.

### 7.4 Security — **done, but for the independent review**
- Argon2id at 64 MiB and two passes by default, configurable, with
  `--tune-argon2` to time the options on the machine itself. Hashes carry
  their parameters, so raising the cost rehashes each account at its next
  sign-in instead of locking anyone out — including every account made before
  this, which were at the crate's 19 MiB minimum.
- Brute-force protection: five wrong passwords or codes lock an account for
  fifteen minutes, right password included, and an address with no account
  locks the same way so the lock reveals nothing. Per-address limits on top —
  30 a minute to the sign-in routes, 600 to anything — with the client address
  read from a trusted proxy's last `X-Forwarded-For` entry, never the first.
- Unlisted publications open from signed, expiring links the author makes:
  an HMAC over the publication, the expiry and a per-publication secret, so
  withdrawing every link is replacing one value. The key travels in a header
  and is taken out of the address bar once read. **Old unlisted links — just
  the id — stop working.**
- Building that found a real gap: access checks run on the blocking thread
  pool, where a request's task-locals do not reach, so the key was never seen
  there. Every database call now carries the request's keys across.
- **Not done: the independent security review.** It has to be independent;
  it is still owed before the first paid customer.

---

## Phase 8 — The writer's desk

### 8.1 Distraction-free writer · **Done**
- A full-screen writing mode that puts away everything but the page: no shell,
  outline, toolbar or panels until the writer asks for them. It builds on
  *1.2*'s focus mode, typewriter scrolling and typography, which stay as they
  are.
- **Done:** *Page* in the editor (or ⌘⇧F) puts away the shell, the outline,
  the header, the toolbar, the goals and every panel, and asks the browser for
  the whole screen. Focus mode, typewriter scrolling and the writer's type
  settings keep working underneath, because the mode is a class on the page
  rather than a different page: the editor, its selection and its undo history
  are untouched on the way in and out. A word count and the save state sit
  faintly in a corner. The pointer at the top edge brings back a slim bar;
  ⌘⇧O slides the outline in over the margin; Escape goes back to the desk.
  A save conflict or being offline with work waiting still shows — hiding
  those is how a writer loses words.

### 8.2 Word-count goals · **Done**
- Daily, weekly and per-project targets, each with a progress indicator. This
  extends *1.2*'s daily target, per-scene target and streak, and *4.4*'s
  widget, rather than starting again.
- **Done:** a weekly target beside the daily one (weeks run Monday to Sunday,
  as the ISO calendar does), and a target for the book with an optional date.
  The goal bar now reads across: this sitting, today, this week, the streak,
  the book, the scene — each target with its own meter. With a date, the book
  says the one thing about a deadline a writer can act on today: how many
  words a day it still asks. The widget shows the week when a weekly target
  is set.
- Changing one goal no longer clears another: `/me/goals` tells a goal sent
  as `null` (clear it) from one left out (leave it alone).
- Found on the way: the target forms have refused most numbers since *1.2*.
  `min=1, step=50` makes 500 invalid — the valid values are 1, 51, 101… — and
  the browser declines to submit without saying why. Any whole number now
  works.

### 8.3 Story bible sketches · **Done**
- Sketches for characters, locations and the like, each started from a
  template, kept with the work and to hand while writing.
- **Done:** a bible entry can be a sketch — named fields in order, started
  from one of seven built-in templates: a main or supporting character, a
  location or a room, an object, a family or organisation, a timeline event.
  Each field carries the question it is asking ("What happened that they are
  still answering?"), shown until it is answered, and a writer can add
  questions of their own. Two new kinds join people, places and notes:
  *things* and *groups*.
- To hand while writing: the bible panel opens with whoever and whatever is
  linked to the scene in front of you, sketch and all, before the rest.

### 8.4 AI summaries · **Done**
- A summarize button for a scene, a chapter or the whole book.
- **Done:** *Summarise* in the editor, for the scene, its chapter or the
  whole book, written by Claude Opus 5.5 through the Messages API. What comes
  back is a suggestion, with a button to put it in the scene's or chapter's
  summary or the book's synopsis; nothing in the manuscript changes otherwise.
- The text goes to Anthropic, which the terms promised it never would. So:
  summaries are off until a writer turns them on, the panel says plainly where
  the text goes before they do, and the terms are now version 2 — everyone is
  asked to agree again. Turned off, no further call is made.
- Each answer is kept against the exact text it came from, so asking again
  costs nothing; new calls are limited per writer; a book too long for one
  request is refused rather than quietly cut short. A manuscript line that
  reads like an instruction is summarised, not obeyed.
- **Verified against a stub of the API, not the API itself.** The one live
  call made here was refused — the key in this environment is not valid — so
  the request shape is checked against the documented API and a recording
  stub, and the live round trip still wants one try with a working key.

### 8.5 UI polish · **Done**
- A more polished interface overall. The row of text buttons across the top of
  the editor becomes a menu, a toolbar, or both.
- **Done:** the header's fifteen buttons are now three groups. Next to the
  save state and the scene's status, a small toolbar holds what you toggle
  while writing (Write / Corkboard, Page, Focus, Aa); three menus — *Scene*,
  *Book*, *People* — open the side panels, tick the one that is open, and
  light up while one of theirs is. The menus work from the keyboard (arrows,
  Home, End, Escape back to the button). Every side panel has a close button.

### 8.6 Context menu on selected text · **Done**
- Right-click (or ⌘-click) on selected text to add a highlight, a bookmark or a
  comment.
- **Done:** in the editor and in the reader, right-click (or ⌘-click, or the
  menu key) on selected text offers *Highlight*, *Bookmark*, *Comment…* and
  *Copy*; on one of your marks, *Remove*. With nothing selected, or with
  Shift held, the browser's own menu — spelling suggestions included — is
  left alone.
- Highlights and bookmarks are yours alone: nobody else is shown them, not
  even the author, and they never enter the manuscript, so the export, the
  phone's editor and the next published version are untouched. On a
  publication they sit at exact positions; on the draft they follow their
  words as you type and are found again by their quote if the text moves
  while you are away. *Book → Highlights & bookmarks* lists them in reading
  order, with a name for a bookmark and a way back to each; the reader has
  the same list above the text. They are in the data export.
- *Comment…* means what a comment already meant in each place: in the reader
  it opens the note for that passage; in the editor, a note on the to-do
  list, which now keeps the passage it was written against.

---

## Sequencing note

Phases 1–3 are the product. Phase 4 is the second client. Phases 5–7 are the
business around it. If the roadmap has to be cut, cut from the bottom: a writing
tool that people trust with a novel earns its way to the rest.
