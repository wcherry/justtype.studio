import type {
  Comment, CommentView, DiffResponse, DocNode, Outline, Publication, PublicationCard,
  RevisionWithContent, SaveResult, Scene, SceneRevision, SceneWithContent, Session, Template,
  TrackedChange,
  User, Version, VersionContent, VersionDetail, Work, WorkKind, Visibility, CommentCategory,
  WritingSummary, EntryView, BibleKind, SplitSceneResponse, ResolveResponse,
  BulkResolveResponse, RestoreResponse, VersionSummary, InviteView, CircleView,
  ReaderProgressView, BlockReactions, ReactionKind, ReviewQuestion, AnswerView,
  QuestionKind, QuestionScope, Standing, BulkStateResponse, ReportReason, TaskView,
  TaskState, ReportView, BlockView, SinceResponse, Profile, FollowState, AuthorCard,
} from './types'

const BASE = '/api'
const ACCESS_KEY = 'scriptorium.access'
const REFRESH_KEY = 'scriptorium.refresh'

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    /** Structured state a client is expected to act on — see the 409 from a scene save. */
    readonly details?: unknown,
  ) {
    super(message)
  }
}

export const tokens = {
  access: () => localStorage.getItem(ACCESS_KEY),
  refresh: () => localStorage.getItem(REFRESH_KEY),
  set(session: Session) {
    localStorage.setItem(ACCESS_KEY, session.accessToken)
    localStorage.setItem(REFRESH_KEY, session.refreshToken)
  },
  clear() {
    localStorage.removeItem(ACCESS_KEY)
    localStorage.removeItem(REFRESH_KEY)
  },
}

/** Serialises refresh attempts so a burst of 401s produces one new token pair. */
let refreshing: Promise<boolean> | null = null

async function tryRefresh(): Promise<boolean> {
  const refreshToken = tokens.refresh()
  if (!refreshToken) return false
  if (!refreshing) {
    refreshing = (async () => {
      try {
        const res = await fetch(`${BASE}/auth/refresh`, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ refreshToken }),
        })
        if (!res.ok) {
          tokens.clear()
          return false
        }
        tokens.set((await res.json()) as Session)
        return true
      } finally {
        refreshing = null
      }
    })()
  }
  return refreshing
}

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
  retry = true,
): Promise<T> {
  const headers: Record<string, string> = {}
  if (body !== undefined) headers['content-type'] = 'application/json'
  const access = tokens.access()
  if (access) headers.authorization = `Bearer ${access}`

  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  })

  if (res.status === 401 && retry && (await tryRefresh())) {
    return request<T>(method, path, body, false)
  }
  if (!res.ok) {
    const payload = await res.json().catch(() => null)
    const error = payload?.error
    throw new ApiError(
      res.status,
      error?.code ?? 'unknown',
      error?.message ?? res.statusText,
      error?.details,
    )
  }
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}

const get = <T>(path: string) => request<T>('GET', path)
const post = <T>(path: string, body?: unknown) => request<T>('POST', path, body ?? {})
const patch = <T>(path: string, body: unknown) => request<T>('PATCH', path, body)
const put = <T>(path: string, body: unknown) => request<T>('PUT', path, body)
const del = <T>(path: string) => request<T>('DELETE', path)

/**
 * Downloads an export.
 *
 * Fetched with the access token in the Authorization header and handed to the
 * browser as a blob, rather than opened as a plain link. A link would need the
 * token in the query string, and URLs end up in server logs, browser history
 * and referrer headers — none of which should ever hold a credential.
 *
 * The cost is holding the file in memory briefly, which for a manuscript is a
 * few megabytes at most.
 */
export function downloadExport(versionId: string, format: string, style: string): Promise<void> {
  return download(`/versions/${versionId}/export`, format, style)
}

/** Every note on a publication, as a document the author can work from. */
export function downloadFeedback(publicationId: string, format: string): Promise<void> {
  return download(`/publications/${publicationId}/feedback/export`, format, 'manuscript')
}

/**
 * Fetched rather than linked, because the token belongs in a header and never
 * in a URL: a query string is logged, kept in history, and handed to whatever
 * the page links to next.
 */
async function download(path: string, format: string, style: string): Promise<void> {
  const params = new URLSearchParams({ format, style })
  const headers: Record<string, string> = {}
  const access = tokens.access()
  if (access) headers.authorization = `Bearer ${access}`

  let res = await fetch(`${BASE}${path}?${params}`, { headers })
  if (res.status === 401 && (await tryRefresh())) {
    const retry = tokens.access()
    if (retry) headers.authorization = `Bearer ${retry}`
    res = await fetch(`${BASE}${path}?${params}`, { headers })
  }
  if (!res.ok) {
    const payload = await res.json().catch(() => null)
    throw new ApiError(
      res.status,
      payload?.error?.code ?? 'unknown',
      payload?.error?.message ?? res.statusText,
    )
  }

  // The server already decided what this file should be called; reading it back
  // keeps one source of truth for the name.
  const disposition = res.headers.get('content-disposition') ?? ''
  const named = /filename="([^"]+)"/.exec(disposition)
  const filename = named?.[1] ?? `manuscript.${format}`

  const url = URL.createObjectURL(await res.blob())
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  document.body.appendChild(link)
  link.click()
  link.remove()
  // Revoked on the next tick: revoking synchronously can beat the download.
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

export const api = {
  goals: {
    /** `today` is the writer's local date; only the client knows it. */
    writing: (today: string, days?: number) =>
      get<WritingSummary>(`/me/writing?today=${today}${days ? `&days=${days}` : ''}`),
    update: (dailyWordGoal: number | null) => patch<User>('/me/goals', { dailyWordGoal }),
  },

  auth: {
    register: (email: string, password: string, displayName: string) =>
      post<Session>('/auth/register', { email, password, displayName }),
    login: (email: string, password: string) =>
      post<Session>('/auth/login', { email, password }),
    logout: () => post<{ ok: boolean }>('/auth/logout', { refreshToken: tokens.refresh() }),
    me: () => get<Session['user']>('/auth/me'),
  },

  templates: {
    list: () => get<Template[]>('/templates'),
  },

  works: {
    list: () => get<Work[]>('/works'),
    get: (workId: string) => get<Work>(`/works/${workId}`),
    outline: (workId: string) => get<Outline>(`/works/${workId}/outline`),
    create: (input: { title: string; kind: WorkKind; templateId?: string; synopsis?: string }) =>
      post<Outline>('/works', input),
    update: (
      workId: string,
      input: Partial<
        Pick<
          Work,
          | 'title' | 'subtitle' | 'synopsis' | 'status'
          | 'penName' | 'shortTitle' | 'dedication' | 'epigraph'
          | 'epigraphSource' | 'copyrightLine'
        >
      >,
    ) => patch<Work>(`/works/${workId}`, input),
    remove: (workId: string) => del<{ ok: boolean }>(`/works/${workId}`),
  },

  chapters: {
    create: (workId: string, title: string) =>
      post<Outline['chapters'][number]>(`/works/${workId}/chapters`, { title }),
    update: (chapterId: string, input: { title?: string; summary?: string }) =>
      patch<Outline['chapters'][number]>(`/chapters/${chapterId}`, input),
    remove: (chapterId: string) => del<{ ok: boolean }>(`/chapters/${chapterId}`),
    reorder: (workId: string, ids: string[]) =>
      post<Outline['chapters']>(`/works/${workId}/chapters/reorder`, { ids }),
  },

  scenes: {
    create: (chapterId: string, title: string) =>
      post<Scene>(`/chapters/${chapterId}/scenes`, { title }),
    get: (sceneId: string) => get<SceneWithContent>(`/scenes/${sceneId}`),
    split: (sceneId: string, atBlock: number, baseHash: string, title?: string) =>
      post<SplitSceneResponse>(`/scenes/${sceneId}/split`, { atBlock, baseHash, title }),
    /** Both hashes, so a merge cannot silently swallow someone else's edit. */
    merge: (sceneId: string, withId: string, baseHash: string, withBaseHash: string) =>
      post<Scene>(`/scenes/${sceneId}/merge`, { with: withId, baseHash, withBaseHash }),
    update: (
      sceneId: string,
      input: Partial<Pick<Scene, 'title' | 'summary' | 'pov' | 'status' | 'chapterId'>> & {
        /** `null` clears the target; omitting leaves it alone. */
        targetWords?: number | null
        location?: string
        storyDate?: string
      },
    ) => patch<Scene>(`/scenes/${sceneId}`, input),
    /** `baseHash` is the contentHash this edit was made on top of; the server
     *  refuses the write if it no longer matches. */
    saveContent: (
      sceneId: string,
      content: DocNode,
      createRevision = true,
      baseHash?: string | null,
      localDay?: string,
    ) => put<SaveResult>(`/scenes/${sceneId}/content`, { content, createRevision, baseHash, localDay }),
    restoreRevision: (sceneId: string, revisionId: string) =>
      post<SaveResult>(`/scenes/${sceneId}/revisions/${revisionId}/restore`),
    remove: (sceneId: string) => del<{ ok: boolean }>(`/scenes/${sceneId}`),
    reorder: (chapterId: string, ids: string[]) =>
      post<Scene[]>(`/chapters/${chapterId}/scenes/reorder`, { ids }),
    revisions: (sceneId: string) => get<SceneRevision[]>(`/scenes/${sceneId}/revisions`),
    revision: (sceneId: string, revisionId: string) =>
      get<RevisionWithContent>(`/scenes/${sceneId}/revisions/${revisionId}`),
  },

  changes: {
    list: (sceneId: string, state = 'pending') =>
      get<TrackedChange[]>(`/scenes/${sceneId}/changes?state=${state}`),
    create: (sceneId: string, input: {
      kind: TrackedChange['kind']; anchorFrom: number; anchorTo: number
      quote?: string; payload?: unknown; note?: string
      /** The scene's contentHash, so the proposal knows what it was written against. */
      baseHash?: string
    }) => post<TrackedChange>(`/scenes/${sceneId}/changes`, input),
    accept: (changeId: string) => post<ResolveResponse>(`/changes/${changeId}/accept`),
    reject: (changeId: string) => post<ResolveResponse>(`/changes/${changeId}/reject`),
    /** Resolve every pending change in a scene, optionally from one author. */
    resolveAll: (sceneId: string, action: 'accept' | 'reject', authorId?: string) =>
      post<BulkResolveResponse>(`/scenes/${sceneId}/changes/resolve`, { action, authorId }),
  },

  versions: {
    list: (workId: string) => get<VersionSummary[]>(`/works/${workId}/versions`),
    create: (
      workId: string,
      label?: string,
      notes?: string,
      scope?: 'work' | 'chapter' | 'scene',
      scopeRefId?: string,
    ) => post<VersionDetail>(`/works/${workId}/versions`, { label, notes, scope, scopeRefId }),
    update: (versionId: string, input: { label?: string; notes?: string }) =>
      patch<VersionDetail>(`/versions/${versionId}`, input),
    addTag: (versionId: string, tag: string) =>
      post<VersionDetail>(`/versions/${versionId}/tags`, { tag }),
    removeTag: (versionId: string, tag: string) =>
      del<VersionDetail>(`/versions/${versionId}/tags/${encodeURIComponent(tag)}`),
    pin: (versionId: string) => post<Work>(`/versions/${versionId}/pin`),
    /** Puts a snapshot back as the working copy, after snapshotting the draft. */
    restore: (versionId: string) => post<RestoreResponse>(`/versions/${versionId}/restore`),
    get: (versionId: string) => get<VersionDetail>(`/versions/${versionId}`),
    content: (versionId: string) => get<VersionContent>(`/versions/${versionId}/content`),
    publish: (versionId: string) => post<Version>(`/versions/${versionId}/publish`),
    finalize: (versionId: string) => post<Version>(`/versions/${versionId}/finalize`),
    diff: (workId: string, from: string, to: string) =>
      get<DiffResponse>(`/works/${workId}/versions/diff?from=${from}&to=${to}`),
  },

  publications: {
    create: (versionId: string, input: {
      visibility: Visibility; scope?: 'work' | 'chapter' | 'scene'
      scopeRefId?: string; reviewPrompt?: string; closesAt?: string
      maxReviewers?: number
      spoilerSafe?: boolean
    }) => post<PublicationCard>(`/versions/${versionId}/publications`, input),
    feed: (opts: { mine?: boolean; following?: boolean; kind?: string; author?: string } = {}) => {
      const params = new URLSearchParams()
      if (opts.mine) params.set('mine', 'true')
      if (opts.following) params.set('following', 'true')
      if (opts.kind) params.set('kind', opts.kind)
      if (opts.author) params.set('author', opts.author)
      const qs = params.toString()
      return get<PublicationCard[]>(`/publications${qs ? `?${qs}` : ''}`)
    },
    get: (publicationId: string) => get<PublicationCard>(`/publications/${publicationId}`),
    content: (publicationId: string) => get<VersionContent>(`/publications/${publicationId}/content`),
    update: (
      publicationId: string,
      input: Partial<Pick<Publication, 'visibility' | 'reviewPrompt' | 'closesAt'>> & {
        /** `0` removes the cap. */
        maxReviewers?: number
        spoilerSafe?: boolean
      },
    ) => patch<PublicationCard>(`/publications/${publicationId}`, input),

    /** Who it was shared with, and whether they have turned up. */
    invites: (publicationId: string) => get<InviteView[]>(`/publications/${publicationId}/invites`),
    invite: (publicationId: string, input: { emails?: string[]; circleId?: string }) =>
      post<InviteView[]>(`/publications/${publicationId}/invites`, input),
    uninvite: (publicationId: string, email: string) =>
      del<{ ok: boolean }>(`/publications/${publicationId}/invites/${encodeURIComponent(email)}`),

    /**
     * What has moved under the notes since this went out. The author's view:
     * it carries everyone's names, and compares against the working draft.
     */
    since: (publicationId: string, to?: string) =>
      get<SinceResponse>(
        `/publications/${publicationId}/since${to ? `?to=${encodeURIComponent(to)}` : ''}`,
      ),

    /** How far each reader got. */
    progress: (publicationId: string) =>
      get<ReaderProgressView[]>(`/publications/${publicationId}/progress`),
    recordProgress: (publicationId: string, sceneId: string) =>
      put<ReaderProgressView>(`/publications/${publicationId}/progress`, { sceneId }),
    remove: (publicationId: string) => del<{ ok: boolean }>(`/publications/${publicationId}`),
  },

  /**
   * One-tap signals and the author's own questions.
   *
   * Reactions come back differently depending on who is asking: a reader gets
   * only their own marks, the author gets the totals. That is the server's
   * rule, not a flag this client passes.
   */
  feedback: {
    reactions: (publicationId: string) =>
      get<BlockReactions[]>(`/publications/${publicationId}/reactions`),
    react: (publicationId: string, input: { sceneId: string; blockIndex: number; kind: ReactionKind }) =>
      put<BlockReactions>(`/publications/${publicationId}/reactions`, input),

    questions: (publicationId: string) =>
      get<ReviewQuestion[]>(`/publications/${publicationId}/questions`),
    ask: (publicationId: string, input: { prompt: string; kind: QuestionKind; scope: QuestionScope }) =>
      post<ReviewQuestion>(`/publications/${publicationId}/questions`, input),
    updateQuestion: (questionId: string, input: { prompt?: string; position?: number }) =>
      patch<ReviewQuestion>(`/questions/${questionId}`, input),
    removeQuestion: (questionId: string) => del<{ deleted: boolean }>(`/questions/${questionId}`),

    answers: (publicationId: string) =>
      get<AnswerView[]>(`/publications/${publicationId}/answers`),
    answer: (
      questionId: string,
      input: { chapterId?: string; rating?: number; body?: string },
    ) => put<AnswerView>(`/questions/${questionId}/answers`, input),

    standing: (userId: string) => get<Standing>(`/users/${userId}/standing`),
  },

  bible: {
    list: (workId: string, opts: { kind?: string; sceneId?: string } = {}) => {
      const params = new URLSearchParams()
      if (opts.kind) params.set('kind', opts.kind)
      if (opts.sceneId) params.set('sceneId', opts.sceneId)
      const qs = params.toString()
      return get<EntryView[]>(`/works/${workId}/bible${qs ? `?${qs}` : ''}`)
    },
    create: (workId: string, input: { kind: BibleKind; name: string; summary?: string; body?: string }) =>
      post<EntryView>(`/works/${workId}/bible`, input),
    update: (entryId: string, input: { name?: string; summary?: string; body?: string }) =>
      patch<EntryView>(`/bible/${entryId}`, input),
    remove: (entryId: string) => del<{ ok: boolean }>(`/bible/${entryId}`),
    link: (entryId: string, sceneId: string) =>
      post<EntryView>(`/bible/${entryId}/links`, { sceneId }),
    unlink: (entryId: string, sceneId: string) =>
      del<EntryView>(`/bible/${entryId}/links/${sceneId}`),
  },

  circles: {
    list: () => get<CircleView[]>('/circles'),
    create: (name: string) => post<CircleView>('/circles', { name }),
    remove: (circleId: string) => del<{ ok: boolean }>(`/circles/${circleId}`),
    addMember: (circleId: string, email: string, name?: string) =>
      post<CircleView>(`/circles/${circleId}/members`, { email, name }),
    removeMember: (circleId: string, email: string) =>
      del<CircleView>(`/circles/${circleId}/members/${encodeURIComponent(email)}`),
  },

  comments: {
    list: (publicationId: string, opts: CommentFilters = {}) => {
      const params = new URLSearchParams()
      if (opts.sceneId) params.set('sceneId', opts.sceneId)
      if (opts.sort) params.set('sort', opts.sort)
      if (opts.category) params.set('category', opts.category)
      if (opts.state) params.set('state', opts.state)
      if (opts.authorId) params.set('authorId', opts.authorId)
      if (opts.minScore != null) params.set('minScore', String(opts.minScore))
      const qs = params.toString()
      return get<CommentView[]>(`/publications/${publicationId}/comments${qs ? `?${qs}` : ''}`)
    },
    create: (publicationId: string, input: {
      body: string; sceneId?: string; parentId?: string
      anchor?: { from: number; to: number }; anchorQuote?: string; category?: CommentCategory
    }) => post<CommentView>(`/publications/${publicationId}/comments`, input),
    update: (commentId: string, input: { body?: string; state?: 'open' | 'resolved' }) =>
      patch<Comment>(`/comments/${commentId}`, input),
    remove: (commentId: string) => del<{ ok: boolean }>(`/comments/${commentId}`),
    vote: (commentId: string, value: -1 | 0 | 1) =>
      post<{ commentId: string; score: number; viewerVote: number }>(`/comments/${commentId}/vote`, { value }),

    /** Resolve or reopen a pile at once. Needs something to narrow it. */
    bulkState: (
      publicationId: string,
      input: {
        state: 'open' | 'resolved'
        commentIds?: string[]
        sceneId?: string
        category?: CommentCategory
        authorId?: string
      },
    ) => post<BulkStateResponse>(`/publications/${publicationId}/comments/state`, input),

    report: (commentId: string, input: { reason: ReportReason; detail?: string }) =>
      post<{ reported: boolean }>(`/comments/${commentId}/report`, input),
  },

  /** The author's list of what to change, and who is not welcome. */
  tasks: {
    list: (workId: string, opts: { state?: 'open' | 'done' | 'all'; sceneId?: string } = {}) => {
      const params = new URLSearchParams()
      if (opts.state) params.set('state', opts.state)
      if (opts.sceneId) params.set('sceneId', opts.sceneId)
      const qs = params.toString()
      return get<TaskView[]>(`/works/${workId}/tasks${qs ? `?${qs}` : ''}`)
    },
    create: (workId: string, input: { body: string; sceneId?: string }) =>
      post<TaskView>(`/works/${workId}/tasks`, input),
    fromComment: (commentId: string) => post<TaskView>(`/comments/${commentId}/task`, {}),
    update: (taskId: string, input: { body?: string; state?: TaskState; sceneId?: string }) =>
      patch<TaskView>(`/tasks/${taskId}`, input),
    remove: (taskId: string) => del<{ deleted: boolean }>(`/tasks/${taskId}`),
  },

  /** Who someone is, what they have on offer, and whether you follow them. */
  profiles: {
    get: (userId: string) => get<Profile>(`/users/${userId}/profile`),
    /** An empty bio clears it. */
    update: (input: { displayName?: string; bio?: string }) => patch<User>('/me/profile', input),
    follow: (userId: string) => put<FollowState>(`/users/${userId}/follow`, {}),
    unfollow: (userId: string) => del<FollowState>(`/users/${userId}/follow`),
    following: () => get<AuthorCard[]>('/me/following'),
  },

  moderation: {
    reports: (publicationId: string) => get<ReportView[]>(`/publications/${publicationId}/reports`),
    blocks: () => get<BlockView[]>('/me/blocks'),
    block: (userId: string) => post<BlockView[]>('/me/blocks', { userId }),
    unblock: (userId: string) => del<BlockView[]>(`/me/blocks/${userId}`),
  },
}

/** What the sidebar is narrowed to. Lifted out because the page owns it. */
export interface CommentFilters {
  sceneId?: string
  sort?: 'top' | 'new'
  category?: CommentCategory
  state?: 'open' | 'resolved'
  authorId?: string
  minScore?: number
}
