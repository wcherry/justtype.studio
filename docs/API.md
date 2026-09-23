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
| `GET` | `/publications` | review feed; `?mine=true`, `?kind=`, `limit`, `offset` |
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
publications they were invited to, and is not shown one whose window has shut.

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

`standing` is derived, never stored: `votes + 3 x resolved` across everything
that reviewer has written. A resolve outweighs an upvote because it is the
author saying they acted on the note rather than a bystander agreeing with it.
The same number rides along on every comment as `authorScore`.

## Comments

| Method | Path | Notes |
|---|---|---|
| `GET` | `/publications/{id}/comments` | threaded; `?sceneId=`, `?sort=top\|new` |
| `POST` | `/publications/{id}/comments` | `body`, `sceneId`, `parentId`, `anchor`, `anchorQuote`, `category` |
| `PATCH` | `/comments/{id}` | `body` (author only), `state` (work owner only) |
| `DELETE` | `/comments/{id}` | soft delete; author or work owner |
| `POST` | `/comments/{id}/vote` | `value`: `1`, `-1`, or `0` to clear |

`anchor` is `{from, to}` in ProseMirror positions against the published
snapshot. `category` is one of `general`, `praise`, `plot`, `pacing`, `prose`,
`character`, `typo`. Sending the same vote value twice clears it. You cannot
vote on your own comment.
