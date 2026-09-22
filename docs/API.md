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
| `PATCH` | `/works/{id}` | `title`, `subtitle`, `synopsis`, `status` (`draft`/`archived` only) |
| `DELETE` | `/works/{id}` | removes the manuscript tree too |
| `POST` | `/works/{id}/chapters` | `title` |
| `POST` | `/works/{id}/chapters/reorder` | `ids` in the order you want |
| `PATCH` | `/chapters/{id}` | `title`, `summary` |
| `DELETE` | `/chapters/{id}` | takes its scenes with it |
| `POST` | `/chapters/{id}/scenes` | `title`, optional `content` |
| `POST` | `/chapters/{id}/scenes/reorder` | `ids` |
| `GET` | `/scenes/{id}` | scene plus its content |
| `PATCH` | `/scenes/{id}` | `title`, `summary`, `pov`, `status`, `chapterId` (to move it) |
| `PUT` | `/scenes/{id}/content` | `content` (a TipTap doc), `createRevision`, `baseHash` |
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
| `POST` | `/scenes/{id}/changes` | `kind`, `anchorFrom`, `anchorTo`, `quote`, `payload`, `note` |
| `POST` | `/changes/{id}/accept` | author only |
| `POST` | `/changes/{id}/reject` | author only |

The server records the proposal and its resolution. Applying an accepted change
to the document is the editor's job — only ProseMirror can map a position
through the edits made since the proposal was written.

## Versions

| Method | Path | Notes |
|---|---|---|
| `GET` | `/works/{id}/versions` | newest first |
| `POST` | `/works/{id}/versions` | `label`, `notes`; snapshots the working copy |
| `GET` | `/works/{id}/versions/diff?from=&to=` | scene-by-scene word diff |
| `GET` | `/versions/{id}` | version plus its manifest entries |
| `GET` | `/versions/{id}/content` | the whole frozen manuscript |
| `POST` | `/versions/{id}/publish` | `draft` → `published` |
| `POST` | `/versions/{id}/finalize` | → `final` |

## Publications

| Method | Path | Notes |
|---|---|---|
| `POST` | `/versions/{id}/publications` | `visibility`, `scope`, `scopeRefId`, `reviewPrompt`, `closesAt` |
| `GET` | `/publications` | review feed; `?mine=true`, `?kind=`, `limit`, `offset` |
| `GET` | `/publications/{id}` | card with author, counts and excerpt |
| `GET` | `/publications/{id}/content` | the snapshot, narrowed to the published scope |
| `PATCH` | `/publications/{id}` | `visibility`, `reviewPrompt`, `closesAt` |
| `DELETE` | `/publications/{id}` | withdraws it, comments included |

`visibility` is `unlisted` (link only), `platform` (signed-in readers) or
`public`. `scope` is `work`, `chapter` or `scene`; the narrower scopes need
`scopeRefId`. A version must be published before it can be offered for review.

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
