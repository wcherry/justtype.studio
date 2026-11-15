# API reference

Base path `/api`. All bodies are JSON with camelCase keys.

Authenticated routes take `Authorization: Bearer <accessToken>`. Errors come
back as `{"error": {"code": "...", "message": "..."}}` with a matching status.

| Code | Status |
|---|---|
| `bad_request` | 400 |
| `unauthorized` | 401 |
| `forbidden` | 403 |
| `not_found` | 404 |
| `conflict` | 409 |
| `internal` | 500 |

## Auth

| Method | Path | Body | Returns |
|---|---|---|---|
| `POST` | `/auth/register` | `email`, `password` (8+), `displayName`, `acceptTerms`, `turnstileToken?` | session |
| `GET` | `/auth/signup-policy` | — | `{ turnstileSiteKey }`: show the Turnstile check with this key, when there is one |
| `POST` | `/auth/login` | `email`, `password` | session |
| `POST` | `/auth/refresh` | `refreshToken` | session (token rotated) |
| `POST` | `/auth/logout` | `refreshToken` | `{ok}` |
| `GET` | `/auth/me` | — | user |

A session is `{user, accessToken, refreshToken, expiresIn}`. Refresh tokens are
single-use: refreshing revokes the one you presented.

### Splitting and merging

A split cuts between top-level blocks, never inside one — `atBlock` is the index
of the first block that belongs to the *new* scene. Block boundaries make the
operation lossless by construction: every block ends up in exactly one half, so
merging what a split produced returns the original document. A merge folds the
named scene into this one, in reading order, and deletes it.

Both require the `contentHash` of every scene involved, and refuse with `409` if
any has moved. Unlike an ordinary save, the hash is not optional: these rewrite
prose in two places at once, and doing that to a document someone else has
edited would be much worse than refusing.

`storyDate` is free text. Fiction rarely runs on the Gregorian calendar, and
"the third day of the siege" is a timeline position too.

## Notebook

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/notebook?kind=&sceneId=` | entries, with the scenes each appears in; each kind in the writer's order (`position`) |
| `POST` | `/works/{id}/notebook` | `kind` (`character`/`place`/`note`), `name`, `summary`, `body`; added at the end of its kind |
| `POST` | `/works/{id}/notebook/reorder` | `kind`, `ids` in their new order (any left out follow, as they were) → that kind's entries |
| `PATCH` | `/notebook/{entryId}` | `name`, `summary`, `body` |
| `DELETE` | `/notebook/{entryId}` | |
| `POST` | `/notebook/{entryId}/links` | `sceneId`; idempotent |
| `DELETE` | `/notebook/{entryId}/links/{sceneId}` | |

Entries link to scenes through a join table rather than a list on either side,
because the question gets asked from both directions — "who is in this scene"
while writing it, and "which scenes is she in" while fixing her eye colour in
chapter nine.

## Export

| Method | Path | Notes |
|---|---|---|
| `GET` | `/versions/{id}/export?format=&style=` | the version as a file |
| `GET` | `/works/{id}/export?format=&style=[&chapters=&scenes=]` | the working draft as a file, or `format=project` for the whole book as a zip (owner); `chapters` and `scenes` (comma-separated ids) narrow it, as printing one scene or chapter does |
| `POST` | `/works/import?name=` | a project zip, a zipped Scrivener project, or a `.md`/`.txt` document, in the body → a new book |
| `POST` | `/works/{id}/import?name=&chapterId=` | a `.md`/`.txt` document in the body → a new scene |

`format` is `docx`, `epub`, `pdf`, `html` or `txt`. `style` is `manuscript` (the
default) or `book`. These GET forms are shorthands that build default
`ExportOptions`; they stay for the iOS app and the smoke test. The response
carries a `Content-Disposition` filename and an `x-export-warnings` count.

### Export options

Everything configurable is one `ExportOptions` document, sent as the JSON body
of the POST forms. Every field has a default, and an empty body is the default
export. Unknown fields, and settings for another format (`pdf` options with
`format: epub`), are refused naming the field.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/works/{id}/export`, `/versions/{id}/export` | the file; records last-used options and a history row |
| `POST` | `…/export/preflight` | `{summary, issues[], ok, filename}` without building the file; errors block, warnings never do |
| `POST` | `…/export/preview[?pages=]` | a web page (first pages for PDF) to look at; recorded nowhere |
| `POST` | `…/export/bundle` | `{formats[], options}` → one zip of every format plus `manifest.json` with per-file warnings |
| `GET` | `/works/{id}/export/last` | `{latest, byFormat[]}` |
| `POST` | `/works/{id}/export/again` | repeats the latest |
| `GET` | `/works/{id}/export/history` | newest first |
| `GET` | `/works/{id}/export/history/{id}/download` | a version export made again; `x-export-identical` says whether it matches the original bytes |
| `GET/POST` | `/export/profiles` | six built-in profiles plus the caller's own |
| `POST` | `/export/profiles/import` | `{name, options}` from a `.json` file; stale settings are reset and listed in `notes` instead of refused |
| `PATCH/DELETE` | `/export/profiles/{id}` | built-ins are read-only |
| `GET/PATCH` | `/works/{id}/metadata` | language, description, publisher, date, identifier, keywords, contributors. Sent to `PATCH /works/{id}` instead, these are refused by name (400) rather than dropped |
| `PUT/GET/DELETE` | `/works/{id}/cover` | a JPEG or PNG in the body (8 MB; checked by its bytes) |

`ExportOptions` = `format`, `content` (`scope` {`kind` all/selected,
`chapterIds`, `sceneIds`}, `frontMatter`, `backMatter`, `toc`, `aboutAuthor`,
`comments` off/footnotes/endnotes, `notePlacement`, `commentNumbering`,
`quotePassage`, `commentLabel`), `style` (`preset` manuscript/book plus
typography overrides, which the manuscript preset ignores), `metadata`
(overrides for this file only), per-format `pdf` (`pageSize`, `marginIn`,
`insideMarginIn` for the gutter, `landscape`, `pageNumbers`,
`pageNumberPosition` bottomCentre/bottomOutside/topOutside,
`pageNumberStyle` plain/dashes, `startPage`, `romanFrontMatter`,
`chaptersOnRight`), `epub`, `html`,
`filenameTemplate` and `profile`. Only the caller's own comments are ever
exported. PDF footnotes are set as notes at the end of each chapter; Word has
real footnotes and endnotes.

Chapters take a `role` on `PATCH /chapters/{id}`: `chapter`, `front`, `back`
or `part`. A part is a divider that opens the chapters after it: it prints on a
page of its own, takes no chapter number (untitled parts are "Part 1", "Part 2",
counted on their own, and chapters number straight through them), and is never
skipped for having no text. A chapter made a part is given an empty scene if it
has none, its part page, since a version keeps a chapter by its scenes; what is
written there (an epigraph, a few lines) prints under the part's title.
`PATCH /me/profile` takes `aboutAuthor`. The project zip round-trips roles, details and the cover.

A version is the export to send: a file you send an agent has to be a thing that
stays the same. The working draft exports too, as it reads now. Scenes with no text are skipped, so
a template's empty beats do not print as pages of lone `#` marks, and a chapter
that is nothing but scaffolding is skipped with them.

### Projects

`format=project` is the book as a zip: `project.json`, `README.txt`, and one
Markdown file per scene under `manuscript/NN-chapter/NN-scene.md`. `project.json`
(`format: "justtype-project"`, `formatVersion: 1`) is the source of truth — the
details of the book, its chapters and scenes with every setting and each scene's
exact document, and the notebook with its links — so importing it makes the same
book. A zip with only Markdown files imports too: a chapter per folder, a scene
per file. So does a zipped Scrivener project (2 or 3) — any zip with a `.scrivx`
in it, which is what the web client makes of a `.scriv` folder: the Draft's
top-level items become chapters (a folder with everything under it flattened
into scenes, a lone document a chapter of one scene), a folder of only folders
becomes a part with its folders as the chapters after it, synopses become summaries,
Scrivener's stock statuses map to ours, and the binder's other folders become
the notebook (Characters and Places by name, everything else notes). The Trash,
template sheets, front and back matter, snapshots, document notes and media are
left behind; formatting comes through as bold, italic, underline and strike.
Imports always make new rows with new ids, start as a draft, and leave
behind what is not part of the writing: versions, readers, comments, to-dos and
collaborators. Import bodies are limited to 64 MB, and a zip to 20,000 files and
256 MB once unpacked.

Front matter — `penName`, `shortTitle`, `dedication`, `epigraph`,
`epigraphSource`, `copyrightLine` — is set on the work; contact details for a
submission's first page (`legalName`, `mailingAddress`, `phone`) belong to the
writer, since they are the same for every submission. Sending an empty string
clears a front-matter field; omitting it leaves it alone.

## Writing goals

| Method | Path | Notes |
|---|---|---|
| `GET` | `/me/writing?today=YYYY-MM-DD&days=30` | today's words, the streak, recent history |
| `PATCH` | `/me/goals` | `dailyWordGoal`; `null` clears it |

`today` and the `localDay` on a scene save are the writer's **local** date. Only
the client knows what day it is where the writer is, and a UTC day would break a
streak for anyone writing in the evening west of Greenwich — precisely the
writer the feature exists to encourage. A save with no `localDay` is not counted
against any day at all: skipping a tally beats crediting the wrong one.

Words are counted **net**, so a day spent cutting reads as the negative number it
is. A day counts toward a streak because the writer showed up, not because the
total went up. Restoring a revision is deliberately not counted — that is
recovering, not writing.

## Templates

| Method | Path | Returns |
|---|---|---|
| `GET` | `/templates` | built-ins plus the caller's own |
| `GET` | `/templates/{id}` | one template with its structure |

## Works, chapters, scenes

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works` | the caller's works, `limit` / `offset` |
| `POST` | `/works` | `title`, `kind`, optional `templateId`; returns the outline |
| `GET` | `/works/{id}` | |
| `GET` | `/works/{id}/outline` | work with chapters and scenes nested; `place` is where the caller was in it, when known |
| `PUT` | `/works/{id}/place` | `sceneId`, `from`, `to`: remember where the caller is, for any device; anyone who can read the work |
| `PATCH` | `/works/{id}` | `title`, `subtitle`, `synopsis`, `status` (`draft`/`archived` only), front matter |
| `DELETE` | `/works/{id}` | removes the manuscript tree too |
| `POST` | `/works/{id}/duplicate` | a copy of the whole book (what a project carries), as a new draft of the caller's titled “… (copy)” |
| `POST` | `/works/{id}/chapters` | `title` |
| `POST` | `/works/{id}/chapters/reorder` | `ids` in the order you want |
| `PATCH` | `/chapters/{id}` | `title`, `summary` |
| `DELETE` | `/chapters/{id}` | takes its scenes with it |
| `POST` | `/chapters/{id}/scenes` | `title`, optional `content` |
| `POST` | `/chapters/{id}/scenes/reorder` | `ids` |
| `GET` | `/scenes/{id}` | scene plus its content |
| `PATCH` | `/scenes/{id}` | `title`, `summary`, `pov`, `status`, `chapterId` (to move it; it goes to the end, then reorder), `targetWords`, `location`, `storyDate`, `excluded` (left out of the book: not exported, not in a version, not in the word count) |
| `POST` | `/scenes/{id}/duplicate` | a copy just after it: text, details and notebook links, titled “… (copy)” |
| `POST` | `/scenes/{id}/split` | `atBlock`, `baseHash`, `title` |
| `POST` | `/scenes/{id}/merge` | `with`, `baseHash`, `withBaseHash` |
| `PUT` | `/scenes/{id}/content` | `content` (a TipTap doc), `createRevision`, `baseHash`, `localDay` |
| `DELETE` | `/scenes/{id}` | |
| `GET` | `/scenes/{id}/revisions` | newest first |
| `GET` | `/scenes/{id}/revisions/{revisionId}` | content plus a diff against the current draft |
| `POST` | `/scenes/{id}/revisions/{revisionId}/restore` | put that revision back as the working copy |

`kind` is one of `novel`, `novella`, `short_story`, `serial`.
Scene `status` is one of the built-in `outline`, `drafting`, `revised`, `done`,
or the `key` of one of the book's own statuses. The outline's `statuses` lists
them all, the book's own first, each with a `label` and a `#rrggbb` `color`.

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/statuses` | the book's own, then the built-in ones |
| `POST` | `/works/{id}/statuses` | `label`, optional `color`; names are unique in the book, built-in ones included |
| `POST` | `/works/{id}/statuses/reorder` | `ids`, the book's own in their new order |
| `PATCH` | `/statuses/{id}` | `label`, `color` |
| `DELETE` | `/statuses/{id}` | its scenes go back to `drafting` |

Each answers with the book's whole list.

### Reviews

A review puts a scene, chapter, part or the whole book in front of readers. It
is a publication of a snapshot: marking something for review reuses the most
recent snapshot of it if the draft has not changed since, and saves a new one
if it has. Owners and writers may manage reviews; a writer handed some
chapters, only of those.

| Method | Path | Notes |
|---|---|---|
| `POST` | `/works/{id}/reviews` | `scope` (`work`, `part`, `chapter`, `scene`), `scopeRefId`, `audience` (`everyone`: anyone signed in; `selected`), `userIds`, `emails`, `prompt`, `closesAt` |
| `GET` | `/works/{id}/reviews` | every review of the book, newest first, with `state` (`open`, `closed`, `revoked`) and `outdated` |
| `PATCH` | `/reviews/{id}` | `audience`, `userIds` with `emails` (the whole list), `prompt`, `closesAt`; `""` clears the last two |
| `POST` | `/reviews/{id}/update` | onto the draft as it is now; comments stay, each with `versionLabel` and `earlierVersion` |
| `POST` | `/reviews/{id}/revoke` | closed to readers; the writers keep every comment |
| `POST` | `/reviews/{id}/reopen` | open again to the same readers; a past closing date is cleared |
| `GET` | `/people?q=` | up to eight people by display name, to pick; never an address |

People picked get an inbox item (`review.invited`); an address gets an email.
Bringing a review up to date tells everyone who has taken part
(`review.updated`). The outline carries `reviews`, the open ones, for badges.

### Save conflicts

`baseHash` is the `contentHash` the client believes it is editing on top of.
When it no longer matches, the save is refused with `409` rather than
overwriting whoever wrote in the meantime:

```json
{ "error": { "code": "conflict", "message": "…", "details": {
  "sceneId": "…", "currentHash": "…", "currentWordCount": 412,
  "updatedAt": "2026-09-21T22:42:15.004Z", "yourHash": "…"
} } }
```

Resolve by re-sending with the `currentHash` the 409 reported — that says "I
have seen their version and still mean this". Omitting `baseHash` skips the
check, which is only right for a scene the client has not read.

A restore is an ordinary save under the hood: it writes a revision of the text
being replaced first, so restoring is itself undoable and history is never
rewritten.

## Tracked changes

| Method | Path | Notes |
|---|---|---|
| `GET` | `/scenes/{id}/changes` | `?state=pending\|accepted\|rejected\|all` |
| `POST` | `/scenes/{id}/changes` | `kind`, `anchorFrom`, `anchorTo`, `quote`, `payload`, `note`, `baseHash` |
| `POST` | `/changes/{id}/accept` | applies it; author only |
| `POST` | `/changes/{id}/reject` | author only |
| `POST` | `/scenes/{id}/changes/resolve` | `action` (`accept`/`reject`), optional `authorId` |

Accepting a change **applies it**. The result is therefore the same from any
client, a batch is one operation rather than twenty round trips through an
editor, and a phone can do it.

Positions are mapped without a transform history:

* a proposal records the `baseHash` it was written against, so an unchanged
  scene applies its positions exactly;
* a scene that has moved on re-anchors by `quote`, which is unambiguous when the
  quotation still appears exactly once in a paragraph;
* anything else is refused as stale and left pending, rather than guessed at.
  `resolve` returns those in `stale` with a reason for each.

Batches apply from the end of the document backwards, so applying one change
never invalidates the positions of the ones still to come.

A change that spans more than one paragraph is refused: joining blocks means
deciding which one's type survives, and such a selection is nearly always an
accident. `format` takes the mark name as its payload; `insert` and `replace`
take the text.

## Versions

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/versions` | newest first |
| `POST` | `/works/{id}/versions` | `label`, `notes`, `scope`, `scopeRefId`; snapshots the working copy |
| `PATCH` | `/versions/{id}` | `label`, `notes` |
| `POST` | `/versions/{id}/tags` | `tag`; idempotent, case-folded |
| `DELETE` | `/versions/{id}/tags/{tag}` | |
| `POST` | `/versions/{id}/pin` | marks it the work's pinned snapshot |
| `POST` | `/versions/{id}/restore` | puts it back as the working copy |
| `GET` | `/works/{id}/versions/diff?from=&to=` | scene-by-scene, paragraph-aligned |
| `GET` | `/versions/{id}` | version plus its manifest entries |
| `GET` | `/versions/{id}/content` | the whole frozen manuscript |
| `POST` | `/versions/{id}/publish` | `draft` → `published` |
| `POST` | `/versions/{id}/finalize` | → `final` |

### Naming, tagging and scope

A snapshot's text is immutable; what you call it is not. `PATCH` renames it and
edits its notes, because a label chosen in the moment ("v3") is rarely the one
that helps six months later ("before the agent's notes").

Tags are free-form and case-folded, so `Submitted` and `submitted` are one tag
and tagging twice is not an error. Pinning is separate and singular — the one
snapshot a writer wants to find without looking, usually what they last sent
out.

`scope` is `work` (the default), `chapter` or `scene`. A narrower snapshot
freezes just that part, for checkpointing a chapter before rewriting it without
implying anything about the rest of the book.

### Restoring

`POST /versions/{id}/restore` puts a snapshot back as the working copy, and
**snapshots the working copy first** — automatically, always. That safety
snapshot is what makes it safe to remove scenes written after the version being
restored; without it, "restore" would quietly discard them. The response names
it, so it can be restored in turn.

A scoped version restores only its own chapter or scene and leaves the rest of
the book alone.

### Diffs

`from` and `to` are version ids, or the literal `working` for the draft as it
stands — "what have I changed since I last snapshotted" is the question writers
ask most often.

Each changed scene is aligned **paragraph by paragraph** rather than diffed as
one run of words across the whole scene, which smeared changes across paragraph
boundaries and made every reflowed line look edited. Each aligned paragraph
comes back as `unchanged`, `modified` (with word-level `changes`), `added`,
`removed`, or `moved`, with `beforeIndex` and `afterIndex` saying where it sat
on each side.

Exact matches are resolved before any similarity pairing. An identical paragraph
elsewhere is a certainty that it moved; similarity is a guess, and letting the
guess win produces both a spurious edit and a spurious insertion.

`GET /scenes/{id}/revisions/{revisionId}` returns the same paragraph-aligned
shape against the scene as it stands now — what restoring it would change.

## Publications

| Method | Path | Notes |
|---|---|---|
| `POST` | `/versions/{id}/publications` | `visibility`, `scope`, `scopeRefId`, `reviewPrompt`, `opensAt`, `closesAt`, `maxReviewers` |
| `GET` | `/publications` | review feed; `?q=`, `?mine=true`, `?following=true`, `?author=`, `?kind=`, `?genre=`, `?length=`, `?window=`, `?sort=`, `limit`, `offset` |
| `GET` | `/publications/{id}` | card with author, counts and excerpt |
| `GET` | `/publications/{id}/content` | the snapshot, narrowed to the published scope; `?chapter=N` for one chapter (from zero) with a `toc` of the whole slice |
| `PATCH` | `/publications/{id}` | `visibility`, `reviewPrompt`, `closesAt`, `maxReviewers` (`0` removes the cap) |
| `DELETE` | `/publications/{id}` | withdraws it, comments included |

`visibility` is `unlisted` (a signed link only — see below), `platform` (signed-in readers),
`invited` (named readers only) or `public`. `scope` is `work`, `chapter` or
`scene`; the narrower scopes need `scopeRefId`. A version must be published
before it can be offered for review.

`opensAt` and `closesAt` bound the review window. Outside it every reader is
refused with `403`, the author excepted — they can always open their own. The
feed applies the same rules it would apply to the click: a reader is shown the
publications they were invited to, and is not shown one whose window has shut
or whose author has blocked them.

### Browsing and the needs-readers queue

The feed narrows by form (`kind`), `genre`, `length` and review `window`. A
value outside the set is a `400`, not an empty list, so a typo cannot pass for
"nothing here". `length` reads the published version's word count:

| `length` | Words |
|---|---|
| `short` | under 7,500 |
| `novelette` | 7,500 – 17,499 |
| `novella` | 17,500 – 39,999 |
| `novel` | 40,000 and up |

`window=closing_soon` is a window that shuts within three days;
`window=open_ended` has no closing date at all.

`sort=needs_readers` is the queue: least-read first, leaving out the caller's
own work and anything they have already opened or left a note on. Each card's
`readerCount` is the distinct people who have opened it or remarked on it, the
author excepted. Reciprocity lifts a publication by its author's
`authorReviewsGiven` — the other people's publications they have left notes on,
each counted once however many notes went on it, and capped at ten so it nudges
rather than buys. Placement is `min(given, 10) − readers`, ties going to fewer
readers and then to whoever has waited longest (`routes/discovery.rs`).

`maxReviewers` caps how many distinct people may comment. Someone already
inside the cap may keep talking; a new voice is refused once it is full.

## Readers

| Method | Path | Notes |
|---|---|---|
| `GET` | `/publications/{id}/invites` | who was invited, and who turned up |
| `POST` | `/publications/{id}/invites` | `emails[]` and/or `circleId` |
| `DELETE` | `/publications/{id}/invites/{email}` | withdraws one invitation |
| `PUT` | `/publications/{id}/progress` | `sceneId`; records the reader's position |
| `GET` | `/publications/{id}/progress` | every reader's furthest point (author only) |
| `GET` | `/circles` | the writer's reader circles, members included |
| `POST` | `/circles` | `name` |
| `DELETE` | `/circles/{id}` | |
| `POST` | `/circles/{id}/members` | `email`, optional `name` |
| `DELETE` | `/circles/{id}/members/{email}` | |

Invitations are addresses, not accounts: the people a writer trusts with a draft
usually have an address and not yet an account here. Addresses are case-folded,
so inviting `Grace@Example.com` twice invites one person. Opening the
publication marks that invitation accepted, which is how the author can tell who
actually turned up.

Progress keeps the **furthest** scene reached, never the latest: a reader who
gave up in chapter three and later skimmed the ending has still stopped in
chapter three. `finishedAt` is set once the furthest point is the last scene.

## Reactions and questions

| Method | Path | Notes |
|---|---|---|
| `GET` | `/publications/{id}/reactions` | a reader's own marks; the author's totals |
| `PUT` | `/publications/{id}/reactions` | `sceneId`, `blockIndex`, `kind` |
| `GET` | `/publications/{id}/questions` | what the author is asking |
| `POST` | `/publications/{id}/questions` | `prompt`, `kind`, `scope` |
| `PATCH` | `/questions/{id}` | `prompt`, `position` |
| `DELETE` | `/questions/{id}` | |
| `PUT` | `/questions/{id}/answers` | `chapterId`, `rating` or `body` |
| `GET` | `/publications/{id}/answers` | the author's: everyone's; a reader's: their own |
| `GET` | `/publications/{id}/feedback/export` | every note as a document; `?format=txt\|docx\|pdf` |
| `GET` | `/publications/{id}/since` | what changed under the notes; `?to=` a version id or `working` |
| `GET` | `/users/{id}/standing` | how useful this reviewer's notes have been |

A reaction is `loved`, `confusing` or `dragging`, anchored to a **block index**
rather than to a selection: two readers never highlight quite the same words,
and a tally nobody can add up is not a signal. One mark per reader per block —
a different kind replaces it, the same kind clears it, the way a repeated vote
does. A block index past the end of the scene is refused.

A reader is shown only their own marks. Not a privacy rule: a reader who can
see that four people found this paragraph confusing is being told what to think
about it, and the fifth mark stops meaning anything.

`kind` is `rating` (1–5) or `text`. `scope` is `work` (asked once) or `chapter`
(asked again for each chapter in the published slice, with the chapter in
`chapterId`). Answering twice corrects the answer rather than adding one.

Reactions and answers are governed by `maxReviewers` exactly as comments are:
the cap is on who may leave feedback, not on which kind.

`since` follows each note's paragraph forward into a newer version — the
working draft by default. Every note comes back as `untouched` (the paragraph
reads exactly as it did), `edited` (you have written it since, so the note is
probably answered), `gone` (the paragraph or the scene is not there any more),
or `unanchored` (a general note, with no paragraph to have changed). A moved
paragraph counts as untouched: the words are the ones the reader read. The
author's view only — it carries everyone's names, and compares against a draft
no reader is entitled to see.

`standing` is derived, never stored: `votes + 3 x resolved` across everything
that reviewer has written. A resolve outweighs an upvote because it is the
author saying they acted on the note rather than a bystander agreeing with it.
The same number rides along on every comment as `authorScore`.

## Comments

| Method | Path | Notes |
|---|---|---|
| `GET` | `/publications/{id}/comments` | threaded; `?sceneId=`, `?sort=top\|new`, `?category=`, `?state=`, `?authorId=`, `?minScore=` |
| `POST` | `/publications/{id}/comments` | `body`, `sceneId`, `parentId`, `anchor`, `anchorQuote`, `category` |
| `PATCH` | `/comments/{id}` | `body` (author only), `state` (work owner only) |
| `DELETE` | `/comments/{id}` | soft delete; author or work owner |
| `POST` | `/comments/{id}/vote` | `value`: `1`, `-1`, or `0` to clear |
| `POST` | `/publications/{id}/comments/state` | bulk resolve or reopen |
| `POST` | `/comments/{id}/report` | `reason`, `detail` |

`anchor` is `{from, to}` in ProseMirror positions against the published
snapshot. `category` is one of `general`, `praise`, `plot`, `pacing`, `prose`,
`character`, `typo`. Sending the same vote value twice clears it. You cannot
vote on your own comment.

Filters apply to the **thread**, not the sentence: a note that matches brings
its replies with it, because hiding a reply would leave an answer without its
question. A bulk state change takes `commentIds`, or a `sceneId`, `category` or
`authorId` to narrow it — a call with none of those is refused rather than
resolving the whole review on one mis-click.

## Revision tasks

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/tasks` | `?state=open\|done\|all`, `?sceneId=` |
| `POST` | `/works/{id}/tasks` | `body`, `sceneId` |
| `POST` | `/comments/{id}/task` | turns a note into work; idempotent |
| `PATCH` | `/tasks/{id}` | `body`, `state`, `sceneId` |
| `DELETE` | `/tasks/{id}` | |

A revision task is the step between forty notes and a plan: the author decides
a note is real and it becomes a line of work against the scene. Deliberately
not a tracked change, which proposes an exact edit to an exact range —
"chapter four needs to be colder" is not a diff. Turning the same comment into
a task twice returns the task already there.

## Moderation

| Method | Path | Notes |
|---|---|---|
| `GET` | `/publications/{id}/reports` | what readers flagged (author only) |
| `GET` | `/me/blocks` | who cannot read your work |
| `POST` | `/me/blocks` | `userId` |
| `DELETE` | `/me/blocks/{userId}` | |

There is no moderation team, so a report goes to the only person with standing
to act on it: the author whose work the comment sits on. Nothing is hidden
automatically — a report is one reader telling the author to look.

A block is one writer's door, not a platform ban. A blocked reader is refused
**any** publication of that author's, checked in `publication_for_reader` so
the rule covers everything rather than the handlers somebody remembered.

Comment creation, voting and reactions are rate limited per user per minute
(`JUSTTYPE_COMMENT_RATE`, `JUSTTYPE_VOTE_RATE`, `JUSTTYPE_REACTION_RATE`;
`0` turns one off). Over the limit is a `429` whose `details.retryAfter` says how
many seconds to wait. The counters live in the process — see `backend/src/limit.rs`.

`spoilerSafe` on a publication withholds notes on scenes a reader has not
reached, using the same furthest-point index that reader progress keeps. It
defaults on for a serial, where not knowing yet is the point of reading one,
and off for anything else. The author always sees everything.

## Profiles and following

| Method | Path | Notes |
|---|---|---|
| `GET` | `/users/{id}/profile` | name, bio, follower counts, standing, what is on offer, review history; signed out is fine |
| `PATCH` | `/me/profile` | `displayName`, `bio` (an empty string clears it) |
| `PUT` | `/users/{id}/follow` | idempotent; `{ following, followers }` |
| `DELETE` | `/users/{id}/follow` | |
| `GET` | `/me/following` | author cards, most recent first |

A profile never carries an email address. Its `publications` are the feed with
`?author=` filled in, so it lists exactly what the viewer could open and no
more. Its `reviews` are the notes this person left on other people's work, but
only on `public` and `platform` publications (or the viewer's own): a note on a
draft shared with named readers would tell strangers the draft exists.

There is no approval step on a follow. What it buys is a feed of work the
author already chose to show, so it reveals nothing they did not offer. An
author who has blocked you cannot be followed.

## Collaboration

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/collaborators` | anyone on the work; addresses only for owners |
| `POST` | `/works/{id}/collaborators` | `email`, `role`, optional `chapterIds` (owners) |
| `PATCH` | `/works/{id}/collaborators/{cid}` | `role`, and `chapterIds` or `wholeWork: true` (owners) |
| `DELETE` | `/works/{id}/collaborators/{cid}` | an owner takes someone off, or you step off yourself |
| `GET` | `/works/{id}/activity` | what happened and who did it, newest first; `limit`, `offset` |

Roles, each including the ones before it:

| Role | May |
|---|---|
| `reader` | open the working draft, its history, its snapshots, its publications |
| `editor` | propose tracked changes; add to and tick off the to-do list |
| `writer` | change the text and the structure; accept or reject proposals; edit the notebook |
| `owner` | snapshots, publishing, publications, front matter, who is on the book |

The person who started the work is an owner and is not a row in the list, so
nothing in it can remove them; they are the only one who can delete the work.

`chapterIds` hands a writer or an editor only some chapters — a ghostwriter, an
editorial hand-off. It narrows what they may **change**, never what they may
read: chapter nine is written against the eight before it. Adding, ordering or
removing chapters needs the whole work. An empty list is allowed and means
nowhere to write, not everywhere; an owner cannot be narrowed.

A collaborator is addressed by email, like an invited reader, and may not have
an account yet. Shared works appear in `GET /works` beside the caller's own,
and every outline carries `access` — `role`, `chapterIds`, `creator` and
`owner` — so a client offers only what the server will accept. The rules are
`support::Access`; every handler that touches a work asks it for an ability.

Activity is written by the handlers as they act rather than derived, because a
derived feed cannot say who removed a chapter. A run of saves to one scene by
one person within half an hour is one line with a `times` count. Collaborators
are named, never shown by address. Co-authors are not counted as readers of
their own book in `readerCount`, and notes on it are not reading for others.

## Account

| Method | Path | Notes |
|---|---|---|
| `POST` | `/auth/register` | now also `acceptTerms: true`, required; mails a link to prove the address |
| `POST` | `/auth/login` | `code` once refused with `two_factor_required` — an authenticator code or a recovery code |
| `POST` | `/me/verification` | mail the proving link again |
| `POST` | `/auth/verify` | `token` from that link |
| `POST` | `/auth/password/forgot` | `email`; the same answer whether or not there is such an account |
| `POST` | `/auth/password/reset` | `token`, `password`; signs out every session |
| `POST` | `/me/password` | `currentPassword`, `newPassword`; signs out every other session |
| `POST` | `/me/two-factor/setup` | a new key and its `otpauth://` link, not yet in force |
| `POST` | `/me/two-factor/enable` | `code` from that key; returns ten recovery codes, once |
| `POST` | `/me/two-factor/disable` | `password` and `code` — both factors to remove one |
| `GET` | `/me/sessions` | every signed-in device, `current` marking this one |
| `DELETE` | `/me/sessions/{id}` | sign one out |
| `POST` | `/me/sessions/sign-out-others` | |
| `PATCH` | `/me/settings` | `doubleEnterSplitsScenes`, `theme` (`system`, `light`, `dark`, `sepia`, `night`); each left out stays as it is; returns the user |
| `GET` | `/me/summary-email` | `often` (`none`, `daily`, `weekly`), `timezone`, `sendTime`, `emailVerified` |
| `PATCH` | `/me/summary-email` | `often`, and the device's IANA `timezone`; turning it on needs a confirmed address (403) |
| `POST` | `/summary-email/unsubscribe?token=` | one click, no sign-in: turns the summary off. Also the email's `List-Unsubscribe-Post` target |
| `POST` | `/invites` | `emails` (up to 10); mails each a fixed invitation from the writer, with a sign-up link. Needs a confirmed address (403); 20 addresses a day (429 for a batch that would cross it). Returns `invited` and `remainingToday`, the same whether or not an address already has an account — one that does is not mailed, and neither is one anybody invited in the last 30 days |

**Invitations wait for a proved address.** A reader invitation and a seat on a
book are both addressed to an email, so until an account has proved its
address, anyone could have typed it in at sign-up and collected them. An
unproved account sees none of what was sent to its address, and opening it is
refused with a message saying to confirm first. Holding a reset link proves the
address too.

**Sessions are real.** Every access token names its session, and the session is
checked on every request, so signing a device out — or resetting a password —
takes effect at once rather than when the token runs out. Refresh tokens rotate
inside a session; signing out ends the session, not just the token in hand. A
token from before sessions existed is refused, and the client's refresh gives
it one.

**Two-factor sign-in** is RFC 6238: thirty-second steps, six digits, one step
of drift either side. A code works once — the step it matched is remembered —
and tries are limited per account per minute. The key counts only once a code
from it has been checked, so a mistyped key cannot lock anyone out.

Links last 72 hours (proving an address) or an hour (a reset), work once, and a
newer one retires the older. Mail to one address is limited to three a minute.

**The summary email** goes at 07:00 in the writer's timezone: daily about the
day before, weekly on Monday about Monday to Sunday before. It holds their net
words (against their goals), new comments from others on reviews of their
books, and their open revision tasks, for books they own or write or edit on.
A period with no words, no comments and nothing ticked off is recorded and not
sent. Only the latest period is ever due, and never one whose send time came
before the writer chose it.

**Not built: OAuth sign-in** (Apple, Google). It needs registered apps and keys.

## Trust

| Method | Path | Notes |
|---|---|---|
| `GET` | `/terms` | `{ version, text }`, the Markdown of `backend/terms.md`; signed out is fine |
| `POST` | `/me/terms` | `version` — agree to the terms in force |
| `GET` | `/me/export` | a zip of everything held about the caller |
| `DELETE` | `/me` | `password`, `code` if two-factor is on, `deleteSharedWorks` |
| `GET` | `/me/audit` | what you did, and what anyone did to a work you own |

The terms say, first, that the writer keeps their copyright. Raising
`TERMS_VERSION` in `routes/privacy.rs` asks everyone to agree again.

The export is a zip: `account.json`, `works.json` (every work you started, with
its outline, snapshots, publications, the feedback on them by name only, notebook,
to-do list, co-authors and activity), `contributions.json` (what you did on
other people's work), `audit.json`, and `manuscripts/` with every file behind
each of your works exactly as stored. No password hash.

Deleting an account deletes the works it started, rows and files both. A work
other people are on is refused with a `409` naming it until the caller says
`deleteSharedWorks`. What the account made inside other people's books is not
cascaded away with it: snapshots, saved revisions and invitations there are
credited to that book's owner; notes are blanked and credited to a placeholder
"former member", so replies under them still have something to answer; votes
are removed and the scores recounted. A removed note that someone answered
stays in the thread as an empty placeholder — which is also now true when
someone deletes their own note.

The audit log records publications offered, changed and withdrawn; readers
invited, uninvited, blocked and unblocked; co-authors added, changed and
removed; works and accounts deleted; and changes to an account's own security.
It has no foreign keys, so the line saying an account was deleted outlives it.

## Operations

| Method | Path | Notes |
|---|---|---|
| `GET` | `/status` | public; `200` when every check passes, `503` otherwise — see `docs/OPERATIONS.md` |
| `GET` | `/.well-known/apple-app-site-association` | (not under `/api`) lets the iOS app open `/prompts/…` links; served when `JUSTTYPE_APPLE_APP_ID` is set |

Every response has an `x-request-id` header, and every error envelope repeats
it as `error.requestId`. A client may send its own (8–64 letters, digits, `-`
or `_`) and it is kept.

## Search

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/search?q=` | the work's working text, best first; anyone who can read the work |
| `GET` | `/publications?q=` | the feed, best match first, each card with a `snippet` |

SQLite FTS5, with stemming (`keeper` finds `keepers`) and the last word
matched as a prefix, for searching as you type. Whatever is typed is turned
into quoted words, so quotes, `AND`, `NEAR` and colons are just words. In a
`snippet`, matched words sit between U+0002 and U+0003 — not HTML, so text
from a manuscript is only ever text.

The feed searches titles, authors, prompts and synopses first and the
published text second, in separate indexes: FTS5 scores a match against the
length of the whole row, and one row holding a title and a novel would bury
every novel's title under its own text. A search ranks, and then the feed's
usual rules decide what the caller may see, so a search never reaches
anything the feed would not show.

Both indexes are brought up to date by the search that reads them: each
indexed row remembers what it was built from (a scene's content hash, a
publication's metadata), and anything that has moved since is re-read. A
scene is written from a dozen places, and an index each of them had to
remember to update would drift.

## Live

| Method | Path | Notes |
|---|---|---|
| `POST` | `/live/ticket` | a single-use ticket, good for thirty seconds; signed out is fine |
| `GET` | `/live?ticket=…` | upgrades to a WebSocket |

A browser cannot put an Authorization header on a WebSocket, and a token in a
URL ends up in logs, so the socket is opened with a ticket fetched over the
ordinary authenticated API instead.

The channel carries notices, never content. Commands are JSON text frames:

| Sent | Meaning |
|---|---|
| `{"type":"join","room":"publication:<id>"}` | hear about a review; the same rules as reading it |
| `{"type":"join","room":"work:<id>","sceneId":"…"}` | hear about a book you can read, and say where you are in it |
| `{"type":"here","room":"work:<id>","sceneId":"…"}` | you moved to another scene |
| `{"type":"leave","room":"…"}` | |

| Received | Meaning |
|---|---|
| `joined`, `denied` (with `message`) | the answer to a join |
| `comments` (`publicationId`) | the notes on a review changed — fetch them again |
| `presence` (`people` in a work room; `readers` in a review) | who is there: by name and scene for co-authors, a number for readers |
| `scene_saved` (`sceneId`, `contentHash`, `by`) | a scene's text changed — a save or an accepted change |

Because a notice only says *that* something changed, a client fetches the
change through the API, and the API's rules — spoilers, filters, blocks —
decide what it sees. Readers of a review are a count, not a list: who else is
reading a draft is not something one reader is owed about another.

In memory and in one process, like the rate limiter. A second process would
put a broker behind `live::Hub`.

## Hardening

**Unlisted links are signed.** An unlisted publication no longer opens for
anyone who has its id. The author makes a link (`POST /publications/{id}/links`
with `days`, 1–90) whose key is an expiry and an HMAC over the publication,
the expiry and a secret kept on the publication; `POST
/publications/{id}/links/revoke` replaces the secret, and every link made so
far stops working. A reader's client sends keys in an `x-share-key` header
(several, comma-separated), never back in a URL. Links made before this
change, which were just the id, no longer open the draft.

**Passwords** are Argon2id at 64 MiB and two passes by default
(`JUSTTYPE_ARGON2_*`); `justtype-backend --tune-argon2 [ms]` times the
options on the machine it runs on. Every hash records its own parameters, so
raising them locks no one out: on the next sign-in the password is checked
against the old hash and stored again under the new cost.

**Sign-in is guarded twice.** Five wrong passwords or codes on one account in
fifteen minutes lock it for the rest of that window — the right password
included, so a lock cannot be used to test guesses — and an address with no
account locks the same way, so a lock reveals nothing. A reset link still
works. Separately, one address may make 30 requests a minute to `/auth/*` and
600 to anything (`JUSTTYPE_AUTH_IP_RATE`, `JUSTTYPE_IP_RATE`); over either
is a `429` with `retryAfter`. Behind a reverse proxy, set
`JUSTTYPE_TRUST_PROXY=true` so the address is the proxy's last
`X-Forwarded-For` entry — never the first, which is whatever the client wrote.

**Sign-up is guarded against scripts** (`signup_guard.rs`). The address must
look like one, and a throwaway-mail domain (a built-in list, plus the domains
staff block; subdomains too) gets the `409` a taken address gets. With
Cloudflare Turnstile configured (`JUSTTYPE_TURNSTILE_SITE_KEY` and
`JUSTTYPE_TURNSTILE_SECRET_KEY`), a valid `turnstileToken` — or an address a
writer invited — makes the sign-up human-verified; a bad token is `400
captcha_failed`; no token is only unverified; Cloudflare unreachable counts as
no token. Limits, counted from `signup_events` so they survive a restart: 3 an
hour and 10 a day per address, 60 an hour across the site, and with Turnstile
on, 2 a day per address and 10 an hour site-wide for unverified sign-ups.
Over one is `429` `signup_rate_limited` or `signup_captcha_required`, with
`retryAfter`. IPv6 addresses count by their /64. A sign-up that fails (taken
address, refused name) does not count. `0` turns a limit off
(`JUSTTYPE_SIGNUP_IP_HOURLY`, `_IP_DAILY`, `_GLOBAL_HOURLY`,
`_UNVERIFIED_IP_DAILY`, `_UNVERIFIED_GLOBAL_HOURLY`).

## Summaries

| Method | Path | Notes |
|---|---|---|
| `GET` | `/features` | `{ summaries, summaryProvider, assistant }` — whether this server can write them |
| `POST` / `DELETE` | `/me/summaries` | turn summaries on or off for this account |
| `POST` | `/scenes/{id}/summary` | one to three sentences |
| `POST` | `/chapters/{id}/summary` | three to five sentences |
| `POST` | `/works/{id}/summary` | a synopsis of the whole book, three to five paragraphs |

Written by Claude (`claude-opus-5-5`, medium effort, server-side refusal
fallbacks) through the Messages API. Three rules hold:

- **Nothing leaves without consent.** Until the account has turned summaries
  on, every call is refused before any text is read, and the terms (version 2)
  say where the text goes when it is on.
- **Each answer is kept** against a digest of the exact text, instructions and
  model it came from; asking again for unchanged text returns the stored
  answer with `cached: true` and costs nothing. New calls are limited to six a
  minute per writer.
- **It is a suggestion.** Nothing is written into the manuscript; the client
  offers to put it in the scene's or chapter's summary, or the book's synopsis.

Changing a scene's text needs `write` on it, and so does summarising it. The
manuscript is sent inside `<manuscript>` tags, and the instructions say that
nothing inside them is an instruction. A book too long for one request (about
850k tokens) is refused, so it is never cut short without saying so.

## The writing assistant

| Method | Path | Notes |
|---|---|---|
| `POST` / `DELETE` | `/me/assistant` | turn the assistant on or off — separately from summaries |
| `POST` | `/scenes/{id}/assist` | `{ task, before, selection, after, instruction }` → `{ task, text, model }` |

`task` is `continue` (from the cursor; `before` must not be empty), `rewrite`
(needs a `selection` and an `instruction`), `proofread` (needs a `selection`)
or `ask` (needs an `instruction`, the question). Same model and fallbacks as
summaries; proofreading runs at low effort, the rest at medium.

- **Its own consent.** Refused with 403 until the account has turned the
  assistant on; agreeing to summaries does not count. The terms say where
  the text goes; turning the assistant on is the agreement, so they stay at
  version 2.
- **The client sends the text**, so unsaved typing is what is read. The
  server keeps the last 24,000 bytes of `before` and the first 4,000 of
  `after`; a `selection` over 12,000 bytes or an `instruction` over 500
  characters is refused. Needs `write` on the scene.
- **Not cached** — asking again is how a writer asks for another try — and
  limited to ten calls a minute per writer.
- **A suggestion.** Nothing is written into the manuscript by the server.

The scene goes inside `<manuscript>` tags (with `<cursor/>` where the caret
is, or `[the passage]` where the selection was), the selection inside
`<passage>`, the writer's request inside `<instruction>`.

## Words

| Method | Path | Notes |
|---|---|---|
| `GET` | `/thesaurus/{word}` | `{ word, base, senses: [{ partOfSpeech, gloss, synonyms, related, antonyms }] }`, or 404 |
| `GET` | `/me/dictionary` | the words you have told the spellchecker are right |
| `PUT` / `DELETE` | `/me/dictionary/{word}` | add or remove one; both return the whole list |

The thesaurus is WordNet 3.1, boiled down by `scripts/build_thesaurus.py` into
`backend/assets/thesaurus.tsv.gz` and compiled into the binary; lookups never
leave the server. An inflected word is found under its base ("walked" →
`walk`, verb senses first). Synonyms and related words are ordered most
common first. Spelling is checked in the browser (nspell, with the Hunspell
American and British English dictionaries under `web/public/dictionaries`);
only the personal dictionary is stored here, at most 5,000 words, and it is in
the data export.

## Highlights and bookmarks

| Method | Path | Notes |
|---|---|---|
| `GET` / `POST` | `/works/{id}/annotations` | your marks on the working draft (`?sceneId=`) |
| `GET` / `POST` | `/publications/{id}/annotations` | your marks on a published version |
| `PATCH` | `/annotations/{id}` | `from`, `to`, `quote` (draft marks only) and `note` |
| `DELETE` | `/annotations/{id}` | |

A mark is `{ sceneId, kind: "highlight" | "bookmark", from, to, quote, note? }`
in ProseMirror positions, like a comment's anchor. Only the person who made a
mark is ever shown it; anyone else gets a 404 for it. Reading the draft or the
publication is enough to mark it. A draft mark's range is a hint: the client
finds the quote again when the text has moved and sends the new range back; a
mark on a publication never moves. At most 2,000 per person per book or
publication; quotes are kept to 2,000 characters, notes to 500.

`POST /works/{id}/tasks` also takes `quote`, the passage a to-do was written
against (it needs `sceneId`).

## Daily Prompts

A prompt a day. A **prompt day** is a calendar day in Pacific time
(`JUSTTYPE_PROMPT_TZ`, default `America/Los_Angeles`), following daylight
saving, written `YYYY-MM-DD`. A day that has not opened is never returned by
any route.

| Method | Path | Notes |
|---|---|---|
| `GET` | `/prompts/today` | `{ prompt, enrolled }`; the prompt carries `opensAt`, `closesAt`, the caller's `myState` (`none` or the entry's state) and `submissions` |
| `GET` | `/prompts?before=&limit=` | the archive, newest first, days before today; `nextBefore` pages back |
| `GET` | `/prompts/{day}` | one day; 404 for one not yet open |
| `GET` / `POST` / `PATCH` / `DELETE` | `/prompts/enrollment` | join (`{ timezone }`, which sets the default email time), change `emailEnabled` / `emailLocalTime` (`HH:MM`) / `emailTimezone` (IANA), leave. Email needs a confirmed address (403). Leaving keeps the preferences for rejoining |
| `POST` | `/prompts/unsubscribe?token=` | one click, no sign-in: turns the email off. Also the email's `List-Unsubscribe-Post` target |
| `GET` / `PUT` / `DELETE` | `/prompts/{day}/entry` | the caller's own answer (404 when not started). `PUT { content, baseHash?, localDay? }` autosaves a private draft, refused with 409 when `baseHash` is stale; words count toward the day's goal as scene saves do. `DELETE` takes every critique and every inbox item about it |
| `POST` | `/prompts/{day}/entry/submit`, `…/withdraw` | submit makes it readable by every enrolled writer; `submittedAt` is the first submission and never moves, and the answer is `late` when that came after its day closed. Editing a submitted answer sets `editedAt`. Withdrawing makes it private again and keeps its critiques |
| `GET` | `/prompts/{day}/entries?sort=newest\|fewest_critiques&cursor=` | submitted answers only, late ones included; 403 unless enrolled |
| `GET` | `/prompt-entries/{id}` | yours in any state, anyone's submitted one while you are enrolled |
| `POST` | `/prompt-entries/{id}/to-work/preview` | the scenes your answer would split into, from your *double Enter* setting; nothing is made |
| `POST` | `/prompt-entries/{id}/to-work` | `{ title? }` → a new work (an outline): one chapter of scenes, the prompt as a notebook note. A copy; the answer is unchanged and no words are counted again |

Writing needs enrollment; reading your own answers does not. A writer who
leaves stops seeing the feed, and their submitted answers stay in it.

**Splitting into scenes.** A line that is only `--` always separates scenes
(also between hard breaks inside a paragraph); `--` in a sentence, an em dash
or `---` does not. A blank paragraph separates them when
`users.doubleEnterSplitsScenes` is on (`PATCH /me/settings`); a run of them is
one break. Empty scenes are dropped. The rule lives in `domain/split.rs`, and
both clients show the preview rather than implementing it.

### Critiques

| Method | Path | Notes |
|---|---|---|
| `GET` / `POST` | `/prompt-entries/{id}/critiques` | the whole tree, flat, oldest first, with `parentId`; `POST { body, parentId? }` (rate-limited by `JUSTTYPE_COMMENT_RATE`) |
| `PATCH` / `DELETE` | `/prompt-critiques/{id}` | edit your own (marked `editedAt`); delete your own, or any on your own answer. One with replies stays as a `removed` placeholder |
| `POST` | `/prompt-critiques/{id}/report` | `{ reason, detail? }`, reasons as for comments; goes to the answer's writer (and their inbox) |
| `GET` | `/prompt-entries/{id}/reports` | the answer's writer only |

Names are always shown. On a submitted answer every enrolled writer sees every
critique; on a withdrawn one its writer sees them all, each critic their own,
and nobody else any.

### Inbox

| Method | Path | Notes |
|---|---|---|
| `GET` | `/inbox?cursor=` | `{ digest, items, nextCursor }`, newest first; `digest` is the newest unread daily summary, pinned |
| `GET` | `/inbox/unread-count` | unread items, plus one for an unread summary |
| `POST` | `/inbox/{id}/read`, `/inbox/digests/{day}/read`, `/inbox/read-all` | each returns the new count |

Items: `prompt.critique` (your answer was critiqued), `prompt.reply` (a reply to
your critique, or anywhere in a thread on your answer: once per event),
`prompt.report`. Nobody is told about their own action. What each kind says
and links to is in `domain/inbox.rs`; adding a kind is a registry entry.

### Admin

| Method | Path | Notes |
|---|---|---|
| `GET` / `POST` | `/admin/prompts` | the queue and the days around today; `POST { body, day? }` queues one or pins it to a future day (409 if taken) |
| `PATCH` / `DELETE` | `/admin/prompts/{id}` | `{ body?, day?, unpin? }`; an edit after the day opened is in the audit log; one with answers cannot be deleted |
| `POST` | `/admin/prompts/reorder` | `{ ids }`, every queued id |
| `GET` | `/admin/jobs` | the job runner's health, recent runs and the mail queue |

Every `/admin/*` route is 403 for anyone who is not site staff, and each asks
for the one permission it needs (`auth::Permission`). Admins are made with
`cargo xtask admin grant <email>` (and `revoke`), never through the API;
moderators are made by an admin from the account page.

### Site administration and moderation

Staff routes (`routes/admin_users.rs`, `admin_restrictions.rs`,
`admin_moderation.rs`). A moderator gets the content half; an admin everything.
A moderator's responses never carry an address, legal name, phone number, IP or
session.

| Method | Path | Notes |
|---|---|---|
| `GET` | `/admin/overview` | the landing page's counts |
| `GET` / `POST` | `/admin/users` | `GET ?q&status&role&verified&sort&cursor`, 50 a page, keyset-paged; `POST { email, displayName, moderator? }` makes an account and mails a setup link (admins) |
| `GET` / `PATCH` / `DELETE` | `/admin/users/{id}` | the whole account; `PATCH { displayName?, bio?, aboutAuthor?, email?, markVerified?, resetTwoFactor?, reason }`; `DELETE { confirmName, reason, deleteSharedWorks?, skipExport? }` |
| `POST` | `/admin/users/{id}/password-reset` `/setup-email` `/sessions/revoke-all` `/deactivate` `/reactivate` `/delete-content` `/notes` `/moderator` | one action each; `DELETE /admin/users/{id}/sessions/{session}` signs one device out |
| `GET` | `/admin/signups` | `?hours=` (24, at most 720): new accounts, the busiest sign-up addresses and email domains (admins) |
| `POST` | `/admin/users/bulk-deactivate` | `{ ip?, emailDomain?, createdAfter?, createdBefore?, includeVerified?, dryRun?, reason? }`: at least one of the first three; a dry run unless `dryRun: false`, which needs `reason`. Never matches staff or the caller; verified addresses only with `includeVerified` (admins) |
| `POST` | `/admin/users/{id}/restrictions` | `{ kind, scope, days?, reasonCode, reasonText, internalNote?, itemIds?, confirmName? }` |
| `PATCH` / `POST` | `/admin/restrictions/{id}` · `/lift` | change the length or message · lift |
| `GET` / `POST` | `/admin/appeals` · `/admin/appeals/{id}/decide` | `{ decision: upheld\|overturned, message, restoreContent? }` |
| `GET` / `POST` | `/admin/permanent-bans` · `/{id}/remove` | the list that outlives accounts; `/admin/blocked-domains` for throwaway domains |
| `GET` | `/admin/queue` | `?tab=content\|accounts\|held\|done&source&reason&surface&mine` |
| `POST` | `/admin/queue/{id}/claim` `/dismiss` `/remove` `/restore` `/hold` `/release` | decisions take `{ reason }`; `/admin/queue/dismiss` is the bulk dismiss |
| `GET` / `POST` / `PATCH` / `DELETE` | `/admin/words` … | the word filter, its allowlist, `/test`, `/import`, `/export`, `/rescan` |
| `GET` / `PUT` / `POST` | `/admin/external` · `/test` · `/admin/moderation-settings` | the platform's settings (secrets are write-only), thresholds, the ladder |
| `GET` | `/admin/log` · `/admin/log.csv` | the moderation log, read from `audit_events`; no route changes a line |

Writer-facing:

| Method | Path | Notes |
|---|---|---|
| `GET` | `/me/restrictions` | where the account stands, with `closing: { purgeAt, daysLeft }` when it will be deleted |
| `POST` | `/me/restrictions/{id}/appeal` | `{ body }`, once per restriction |
| `GET` / `POST` | `/ban/appeal/{token}` · `/ban/appeal` | for someone who can't sign in: the link in the ban email |
| `POST` / `GET` | `/ban/export` · `/ban/export/{token}` | a copy of the work, mailed to a permanently banned account until it is purged |
| `POST` | `/auth/setup` | `{ token, password, acceptTerms }`: following a staff-made account's link |
| `POST` | `/reports` | `{ subjectType: entry\|question\|answer\|profile\|publication, subjectId, reason, detail? }`; comments and critiques keep their own routes and also reach staff |
| `POST` | `/webhooks/moderation/{provider}` | the external platform's events; signed (`x-signature`, HMAC-SHA256 of the body), stored, applied by a job |

Refusals have their own codes, both 403 with `details`:
`account_banned` (at sign-in and on every request: `until`, `reason`,
`reasonCode`) and `account_restricted` (`capability`, `until`, `reason`). Posts
that are held, removed by a moderator, or awaiting review carry
`moderationState` (`visible`, `held`, `removed`).
