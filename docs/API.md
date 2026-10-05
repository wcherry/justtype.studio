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
| `POST` | `/auth/register` | `email`, `password` (8+), `displayName` | session |
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

## Story bible

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/bible?kind=&sceneId=` | entries, with the scenes each appears in |
| `POST` | `/works/{id}/bible` | `kind` (`character`/`place`/`note`), `name`, `summary`, `body` |
| `PATCH` | `/bible/{entryId}` | `name`, `summary`, `body` |
| `DELETE` | `/bible/{entryId}` | |
| `POST` | `/bible/{entryId}/links` | `sceneId`; idempotent |
| `DELETE` | `/bible/{entryId}/links/{sceneId}` | |

Entries link to scenes through a join table rather than a list on either side,
because the question gets asked from both directions — "who is in this scene"
while writing it, and "which scenes is she in" while fixing her eye colour in
chapter nine.

## Export

| Method | Path | Notes |
|---|---|---|
| `GET` | `/versions/{id}/export?format=&style=` | the version as a file |

`format` is `docx`, `epub`, `pdf` or `txt`. `style` is `manuscript` (the
default) or `book`, and applies to DOCX only — EPUB has no pages, and the PDF
writer is Courier-only, which is what standard manuscript format asks for
anyway. The response carries a `Content-Disposition` filename derived from the
title.

Exports come from a frozen version, never the working copy: a file you send an
agent has to be a thing that stays the same. Scenes with no text are skipped, so
a template's empty beats do not print as pages of lone `#` marks, and a chapter
that is nothing but scaffolding is skipped with them.

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
| `GET` | `/works/{id}/outline` | work with chapters and scenes nested |
| `PATCH` | `/works/{id}` | `title`, `subtitle`, `synopsis`, `status` (`draft`/`archived` only), front matter |
| `DELETE` | `/works/{id}` | removes the manuscript tree too |
| `POST` | `/works/{id}/chapters` | `title` |
| `POST` | `/works/{id}/chapters/reorder` | `ids` in the order you want |
| `PATCH` | `/chapters/{id}` | `title`, `summary` |
| `DELETE` | `/chapters/{id}` | takes its scenes with it |
| `POST` | `/chapters/{id}/scenes` | `title`, optional `content` |
| `POST` | `/chapters/{id}/scenes/reorder` | `ids` |
| `GET` | `/scenes/{id}` | scene plus its content |
| `PATCH` | `/scenes/{id}` | `title`, `summary`, `pov`, `status`, `chapterId` (to move it), `targetWords`, `location`, `storyDate` |
| `POST` | `/scenes/{id}/split` | `atBlock`, `baseHash`, `title` |
| `POST` | `/scenes/{id}/merge` | `with`, `baseHash`, `withBaseHash` |
| `PUT` | `/scenes/{id}/content` | `content` (a TipTap doc), `createRevision`, `baseHash`, `localDay` |
| `DELETE` | `/scenes/{id}` | |
| `GET` | `/scenes/{id}/revisions` | newest first |
| `GET` | `/scenes/{id}/revisions/{revisionId}` | content plus a diff against the current draft |
| `POST` | `/scenes/{id}/revisions/{revisionId}/restore` | put that revision back as the working copy |

`kind` is one of `novel`, `novella`, `short_story`, `serial`.
Scene `status` is one of `outline`, `drafting`, `revised`, `done`.

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
| `GET` | `/publications` | review feed; `?mine=true`, `?following=true`, `?author=`, `?kind=`, `?genre=`, `?length=`, `?window=`, `?sort=`, `limit`, `offset` |
| `GET` | `/publications/{id}` | card with author, counts and excerpt |
| `GET` | `/publications/{id}/content` | the snapshot, narrowed to the published scope |
| `PATCH` | `/publications/{id}` | `visibility`, `reviewPrompt`, `closesAt`, `maxReviewers` (`0` removes the cap) |
| `DELETE` | `/publications/{id}` | withdraws it, comments included |

`visibility` is `unlisted` (link only), `platform` (signed-in readers),
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
(`SCRIPTORIUM_COMMENT_RATE`, `SCRIPTORIUM_VOTE_RATE`, `SCRIPTORIUM_REACTION_RATE`;
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
| `writer` | change the text and the structure; accept or reject proposals; edit the bible |
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

**Not built: OAuth sign-in** (Apple, Google). It needs registered apps and keys.

## Trust

| Method | Path | Notes |
|---|---|---|
| `GET` | `/terms` | `{ version, text }`, the Markdown of `docs/TERMS.md`; signed out is fine |
| `POST` | `/me/terms` | `version` — agree to the terms in force |
| `GET` | `/me/export` | a zip of everything held about the caller |
| `DELETE` | `/me` | `password`, `code` if two-factor is on, `deleteSharedWorks` |
| `GET` | `/me/audit` | what you did, and what anyone did to a work you own |

The terms say, first, that the writer keeps their copyright. Raising
`TERMS_VERSION` in `routes/privacy.rs` asks everyone to agree again.

The export is a zip: `account.json`, `works.json` (every work you started, with
its outline, snapshots, publications, the feedback on them by name only, bible,
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

Every response has an `x-request-id` header, and every error envelope repeats
it as `error.requestId`. A client may send its own (8–64 letters, digits, `-`
or `_`) and it is kept.
