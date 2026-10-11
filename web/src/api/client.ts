import type {
  Comment, CommentView, DiffResponse, DocNode, Outline, Publication, PublicationCard,
  RevisionWithContent, SaveResult, Scene, SceneRevision, SceneWithContent, Session, Template,
  TrackedChange,
  User, Version, VersionContent, VersionDetail, Work, WorkKind, Visibility, CommentCategory,
  WritingSummary, EntryView, NotebookKind, SplitSceneResponse, ResolveResponse,
  BulkResolveResponse, RestoreResponse, VersionSummary, InviteView, CircleView,
  ReaderProgressView, BlockReactions, ReactionKind, ReviewQuestion, AnswerView,
  QuestionKind, QuestionScope, Standing, BulkStateResponse, ReportReason, TaskView,
  TaskState, ReportView, BlockView, SinceResponse, Profile, FollowState, AuthorCard,
  Genre, LengthBand, WindowFilter, FeedSort, CollaboratorRole, CollaboratorView, ActivityView,
  SessionView, TwoFactorSetup, RecoveryCodes, AuditEvent, Terms, StatusReport, SceneHit, ShareLink,
  SketchField, SketchTemplate, SummaryView, Features, Annotation, AnnotationKind,
  ExportOptions, ExportFormat, PreflightReport, ExportProfile, ExportProfiles, LastUsedResponse,
  ExportHistoryItem, MetadataResponse, UpdateMetadataRequest,
  AssistRequest, AssistView, ThesaurusEntry,
  AdminPrompts, ArchivePage, CritiqueReportView, CritiqueView, EnrollmentView, EntryPage,
  InboxPage, JobsReport, PromptEntryView, PromptView, TodayResponse, ToWorkPreview, UnreadCount,
  UpdateEnrollmentRequest, UpdateSettingsRequest, WritingPlace, InviteResponse,
  AccountStanding, AppealRow, BlockedDomain, ExternalTestResult, ExternalView,
  LogPage, ModerationSettings, MyAppeal, MyRestriction, NoteView, Overview, PermanentBan,
  QueueItem, QueuePage, Reported, RestrictionView, TestResult, UserDetail, UserList,
  WordFilterTerm, WordList, Appeal,
} from './types'

const BASE = '/api'
const ACCESS_KEY = 'justtype.access'
const REFRESH_KEY = 'justtype.refresh'

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    /** Structured state a client is expected to act on — see the 409 from a scene save. */
    readonly details?: unknown,
    /** The id on the server's log lines for this request, to quote. */
    readonly requestId?: string,
  ) {
    // A failure on our side is one the writer cannot fix, so it says what to
    // quote to someone who can.
    super(status >= 500 && requestId ? `${message} (reference ${requestId})` : message)
  }
}

const SHARE_KEYS = 'justtype.shareKeys'

/**
 * Keys from links to unlisted drafts, kept for this tab and sent as a header
 * on every request — never put back into a URL, where they would be logged.
 */
export const shareKeys = {
  all(): string[] {
    try {
      return JSON.parse(sessionStorage.getItem(SHARE_KEYS) ?? '[]') as string[]
    } catch {
      return []
    }
  },
  remember(key: string) {
    const keys = [key, ...shareKeys.all().filter((k) => k !== key)].slice(0, 10)
    try {
      sessionStorage.setItem(SHARE_KEYS, JSON.stringify(keys))
    } catch {
      // Private browsing: the key still works for this page's requests.
    }
  },
  header(): Record<string, string> {
    const keys = shareKeys.all()
    return keys.length > 0 ? { 'x-share-key': keys.join(',') } : {}
  },
}

/** Ours, so a failure can be matched to the server's log before it answers. */
function newRequestId(): string {
  const random = typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID().replace(/-/g, '')
    : Math.random().toString(16).slice(2) + Date.now().toString(16)
  return `web-${random.slice(0, 20)}`
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
  const headers: Record<string, string> = { 'x-request-id': newRequestId(), ...shareKeys.header() }
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
      error?.requestId ?? res.headers.get('x-request-id') ?? undefined,
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

/** The working draft as it stands now, as a file. For a copy that stays put, export a version. */
export function downloadDraftExport(workId: string, format: string, style: string): Promise<void> {
  return download(`/works/${workId}/export`, format, style)
}

/**
 * Sends a file as the body of a request, as it is — a project zip or a
 * Markdown document — rather than wrapped in JSON.
 */
async function upload<T>(path: string, file: File): Promise<T> {
  const send = () => {
    const headers: Record<string, string> = { 'content-type': 'application/octet-stream' }
    const access = tokens.access()
    if (access) headers.authorization = `Bearer ${access}`
    return fetch(`${BASE}${path}`, { method: 'POST', headers, body: file })
  }
  let res = await send()
  if (res.status === 401 && (await tryRefresh())) res = await send()
  if (!res.ok) {
    const payload = await res.json().catch(() => null)
    throw new ApiError(
      res.status,
      payload?.error?.code ?? 'unknown',
      payload?.error?.message ?? res.statusText,
    )
  }
  return (await res.json()) as T
}

/** Every note on a publication, as a document the author can work from. */
/** Everything held about the caller, as one zip. */
export async function downloadAccountExport(): Promise<void> {
  const headers: Record<string, string> = {}
  let access = tokens.access()
  if (access) headers.authorization = `Bearer ${access}`
  let res = await fetch(`${BASE}/me/export`, { headers })
  if (res.status === 401 && (await tryRefresh())) {
    access = tokens.access()
    if (access) headers.authorization = `Bearer ${access}`
    res = await fetch(`${BASE}/me/export`, { headers })
  }
  if (!res.ok) {
    const payload = await res.json().catch(() => null)
    throw new ApiError(res.status, payload?.error?.code ?? 'unknown', payload?.error?.message ?? res.statusText)
  }
  const url = URL.createObjectURL(await res.blob())
  const link = document.createElement('a')
  link.href = url
  link.download = 'justtype-studio-export.zip'
  document.body.appendChild(link)
  link.click()
  link.remove()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
}

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

export interface AnnotationInput {
  sceneId: string
  kind: AnnotationKind
  from: number
  to: number
  quote: string
  note?: string
}

export const api = {
  goals: {
    /** `today` is the writer's local date; only the client knows it. */
    writing: (today: string, days?: number) =>
      get<WritingSummary>(`/me/writing?today=${today}${days ? `&days=${days}` : ''}`),
    /** `null` clears a goal; leaving one out leaves it alone. */
    update: (input: { dailyWordGoal?: number | null; weeklyWordGoal?: number | null }) =>
      patch<User>('/me/goals', input),
  },

  auth: {
    register: (email: string, password: string, displayName: string, acceptTerms: boolean) =>
      post<Session>('/auth/register', { email, password, displayName, acceptTerms }),
    /** `code` only once the server has refused with `two_factor_required`. */
    login: (email: string, password: string, code?: string) =>
      post<Session>('/auth/login', { email, password, code }),
    logout: () => post<{ ok: boolean }>('/auth/logout', { refreshToken: tokens.refresh() }),
    me: () => get<Session['user']>('/auth/me'),
  },

  templates: {
    list: () => get<Template[]>('/templates'),
  },

  works: {
    list: () => get<Work[]>('/works'),
    /** A project zip becomes a book; a Markdown or text document, a book of one scene. */
    import: (file: File) => upload<Outline>(`/works/import?name=${encodeURIComponent(file.name)}`, file),
    /** A Markdown or text document becomes a new scene, in `chapterId` or the last chapter. */
    importDocument: (workId: string, file: File, chapterId?: string) =>
      upload<Scene>(
        `/works/${workId}/import?name=${encodeURIComponent(file.name)}${
          chapterId ? `&chapterId=${chapterId}` : ''
        }`,
        file,
      ),
    get: (workId: string) => get<Work>(`/works/${workId}`),
    outline: (workId: string) => get<Outline>(`/works/${workId}/outline`),
    /** Where the writer is in the book, so opening it again — anywhere — goes back there. */
    putPlace: (workId: string, place: WritingPlace) => put<WritingPlace>(`/works/${workId}/place`, place),
    create: (input: {
      title: string; kind: WorkKind; templateId?: string; synopsis?: string; genre?: Genre
    }) => post<Outline>('/works', input),
    update: (
      workId: string,
      input: Partial<
        Pick<
          Work,
          | 'title' | 'subtitle' | 'synopsis' | 'status'
          | 'penName' | 'shortTitle' | 'dedication' | 'epigraph'
          | 'epigraphSource' | 'copyrightLine'
        >
      > & {
        /** An empty string clears it. */
        genre?: Genre | ''
        /** `0` clears it. */
        targetWords?: number
        /** `YYYY-MM-DD`; an empty string clears it. */
        targetDate?: string
      },
    ) => patch<Work>(`/works/${workId}`, input),
    remove: (workId: string) => del<{ ok: boolean }>(`/works/${workId}`),
    search: (workId: string, q: string) =>
      get<SceneHit[]>(`/works/${workId}/search?q=${encodeURIComponent(q)}`),
  },

  chapters: {
    create: (workId: string, title: string) =>
      post<Outline['chapters'][number]>(`/works/${workId}/chapters`, { title }),
    update: (chapterId: string, input: { title?: string; summary?: string; role?: 'chapter' | 'front' | 'back' }) =>
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
    feed: (opts: FeedFilters & { mine?: boolean; following?: boolean; author?: string; q?: string } = {}) => {
      const params = new URLSearchParams()
      if (opts.q?.trim()) params.set('q', opts.q.trim())
      if (opts.mine) params.set('mine', 'true')
      if (opts.following) params.set('following', 'true')
      if (opts.author) params.set('author', opts.author)
      if (opts.kind) params.set('kind', opts.kind)
      if (opts.genre) params.set('genre', opts.genre)
      if (opts.length) params.set('length', opts.length)
      if (opts.window) params.set('window', opts.window)
      if (opts.sort) params.set('sort', opts.sort)
      const qs = params.toString()
      return get<PublicationCard[]>(`/publications${qs ? `?${qs}` : ''}`)
    },
    get: (publicationId: string) => get<PublicationCard>(`/publications/${publicationId}`),
    /** With `chapter`, one chapter and the table of contents; without, everything. */
    content: (publicationId: string, chapter?: number) =>
      get<VersionContent>(
        `/publications/${publicationId}/content${chapter === undefined ? '' : `?chapter=${chapter}`}`,
      ),
    update: (
      publicationId: string,
      input: Partial<Pick<Publication, 'visibility' | 'reviewPrompt' | 'closesAt'>> & {
        /** `0` removes the cap. */
        maxReviewers?: number
        spoilerSafe?: boolean
      },
    ) => patch<PublicationCard>(`/publications/${publicationId}`, input),

    /** A signed link to an unlisted draft, good for `days`. */
    makeLink: (publicationId: string, days: number) =>
      post<ShareLink>(`/publications/${publicationId}/links`, { days }),
    /** Every link made so far stops working. */
    revokeLinks: (publicationId: string) =>
      post<{ revoked: boolean }>(`/publications/${publicationId}/links/revoke`),

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

  notebook: {
    list: (workId: string, opts: { kind?: string; sceneId?: string } = {}) => {
      const params = new URLSearchParams()
      if (opts.kind) params.set('kind', opts.kind)
      if (opts.sceneId) params.set('sceneId', opts.sceneId)
      const qs = params.toString()
      return get<EntryView[]>(`/works/${workId}/notebook${qs ? `?${qs}` : ''}`)
    },
    templates: () => get<SketchTemplate[]>('/notebook/templates'),
    create: (workId: string, input: {
      kind: NotebookKind; name: string; summary?: string; body?: string; templateId?: string
    }) => post<EntryView>(`/works/${workId}/notebook`, input),
    update: (entryId: string, input: {
      name?: string; summary?: string; body?: string; fields?: SketchField[]
    }) => patch<EntryView>(`/notebook/${entryId}`, input),
    remove: (entryId: string) => del<{ ok: boolean }>(`/notebook/${entryId}`),
    link: (entryId: string, sceneId: string) =>
      post<EntryView>(`/notebook/${entryId}/links`, { sceneId }),
    unlink: (entryId: string, sceneId: string) =>
      del<EntryView>(`/notebook/${entryId}/links/${sceneId}`),
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
    create: (workId: string, input: { body: string; sceneId?: string; quote?: string }) =>
      post<TaskView>(`/works/${workId}/tasks`, input),
    fromComment: (commentId: string) => post<TaskView>(`/comments/${commentId}/task`, {}),
    update: (taskId: string, input: { body?: string; state?: TaskState; sceneId?: string }) =>
      patch<TaskView>(`/tasks/${taskId}`, input),
    remove: (taskId: string) => del<{ deleted: boolean }>(`/tasks/${taskId}`),
  },

  /** Proving the address, getting back in, the second factor, sessions. */
  account: {
    resendVerification: () => post<{ ok: boolean }>('/me/verification'),
    verify: (token: string) => post<{ ok: boolean }>('/auth/verify', { token }),
    forgot: (email: string) => post<{ ok: boolean }>('/auth/password/forgot', { email }),
    reset: (token: string, password: string) =>
      post<{ ok: boolean }>('/auth/password/reset', { token, password }),
    changePassword: (currentPassword: string, newPassword: string) =>
      post<{ ok: boolean }>('/me/password', { currentPassword, newPassword }),
    setupTwoFactor: () => post<TwoFactorSetup>('/me/two-factor/setup'),
    enableTwoFactor: (code: string) => post<RecoveryCodes>('/me/two-factor/enable', { code }),
    disableTwoFactor: (password: string, code: string) =>
      post<{ ok: boolean }>('/me/two-factor/disable', { password, code }),
    sessions: () => get<SessionView[]>('/me/sessions'),
    revokeSession: (sessionId: string) => del<{ ok: boolean }>(`/me/sessions/${sessionId}`),
    signOutOthers: () => post<{ ok: boolean }>('/me/sessions/sign-out-others'),
    /** Settings every device reads from the account. */
    updateSettings: (input: UpdateSettingsRequest) => patch<User>('/me/settings', input),
    /** Ask people to come and write here, by email. */
    invite: (emails: string[]) => post<InviteResponse>('/invites', { emails }),
  },

  /** What this server can do. */
  features: () => get<Features>('/features'),

  /**
   * Summaries written by Claude. Off until the writer turns them on, because
   * turning them on sends the text they ask about to Anthropic.
   */
  summaries: {
    allow: () => post<User>('/me/summaries'),
    withdraw: () => del<User>('/me/summaries'),
    scene: (sceneId: string) => post<SummaryView>(`/scenes/${sceneId}/summary`),
    chapter: (chapterId: string) => post<SummaryView>(`/chapters/${chapterId}/summary`),
    work: (workId: string) => post<SummaryView>(`/works/${workId}/summary`),
  },

  /**
   * The writing assistant, also Claude, and off until the writer turns it on
   * — separately from summaries — because it sends what they hand it to
   * Anthropic.
   */
  assistant: {
    allow: () => post<User>('/me/assistant'),
    withdraw: () => del<User>('/me/assistant'),
    assist: (sceneId: string, input: AssistRequest) =>
      post<AssistView>(`/scenes/${sceneId}/assist`, input),
  },

  /**
   * The thesaurus, served by this app's own server, and the words the writer
   * has told the spellchecker are right. Spelling itself is checked in the
   * browser; neither sends a word anywhere else.
   */
  words: {
    thesaurus: (word: string) => get<ThesaurusEntry>(`/thesaurus/${encodeURIComponent(word)}`),
    dictionary: () => get<string[]>('/me/dictionary'),
    addToDictionary: (word: string) => put<string[]>(`/me/dictionary/${encodeURIComponent(word)}`, {}),
    removeFromDictionary: (word: string) => del<string[]>(`/me/dictionary/${encodeURIComponent(word)}`),
  },

  /**
   * Your own highlights and bookmarks. Nobody else sees them. On the working
   * draft they belong to the book; on a published version, to the publication.
   */
  annotations: {
    draft: (workId: string) => get<Annotation[]>(`/works/${workId}/annotations`),
    markDraft: (workId: string, input: AnnotationInput) =>
      post<Annotation>(`/works/${workId}/annotations`, input),
    publication: (publicationId: string) =>
      get<Annotation[]>(`/publications/${publicationId}/annotations`),
    markPublication: (publicationId: string, input: AnnotationInput) =>
      post<Annotation>(`/publications/${publicationId}/annotations`, input),
    update: (id: string, input: { from?: number; to?: number; quote?: string; note?: string }) =>
      patch<Annotation>(`/annotations/${id}`, input),
    remove: (id: string) => del<void>(`/annotations/${id}`),
  },

  /** Whether the service is fit to serve. Public. */
  status: async (): Promise<StatusReport> => {
    // A 503 still carries the report, and the report is the point.
    const res = await fetch(`${BASE}/status`)
    return (await res.json()) as StatusReport
  },

  /** The terms, the record, and leaving. */
  trust: {
    terms: () => get<Terms>('/terms'),
    acceptTerms: (version: number) => post<User>('/me/terms', { version }),
    audit: () => get<AuditEvent[]>('/me/audit'),
    /** Refused with a 409 naming them if works with co-authors would go too. */
    deleteAccount: (input: { password: string; code?: string; deleteSharedWorks?: boolean }) =>
      request<{ worksDeleted: number }>('DELETE', '/me', input),
  },

  /** Who else is on a work, what they may do, and what they have done. */
  collaborators: {
    list: (workId: string) => get<CollaboratorView[]>(`/works/${workId}/collaborators`),
    /** `chapterIds` narrows what they may change; omitted, the whole work. */
    add: (workId: string, input: { email: string; role: CollaboratorRole; chapterIds?: string[] }) =>
      post<CollaboratorView[]>(`/works/${workId}/collaborators`, input),
    update: (
      workId: string,
      collaboratorId: string,
      input: { role?: CollaboratorRole; chapterIds?: string[]; wholeWork?: boolean },
    ) => patch<CollaboratorView[]>(`/works/${workId}/collaborators/${collaboratorId}`, input),
    /** Take someone off, or step off yourself. */
    remove: (workId: string, collaboratorId: string) =>
      del<{ removed: boolean }>(`/works/${workId}/collaborators/${collaboratorId}`),
    activity: (workId: string, limit = 50) =>
      get<ActivityView[]>(`/works/${workId}/activity?limit=${limit}`),
  },

  /** Who someone is, what they have on offer, and whether you follow them. */
  profiles: {
    get: (userId: string) => get<Profile>(`/users/${userId}/profile`),
    /** An empty bio clears it. */
    update: (input: { displayName?: string; bio?: string; aboutAuthor?: string }) =>
      patch<User>('/me/profile', input),
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
/** Daily Prompts: the day's prompt, taking part, an answer, everyone else's. */
export const prompts = {
  today: () => get<TodayResponse>('/prompts/today'),
  day: (day: string) => get<PromptView>(`/prompts/${day}`),
  archive: (before?: string, limit = 30) =>
    get<ArchivePage>(`/prompts?limit=${limit}${before ? `&before=${encodeURIComponent(before)}` : ''}`),

  enrollment: () => get<EnrollmentView>('/prompts/enrollment'),
  join: (timezone: string) => post<EnrollmentView>('/prompts/enrollment', { timezone }),
  updateEnrollment: (input: UpdateEnrollmentRequest) => patch<EnrollmentView>('/prompts/enrollment', input),
  leave: () => del<EnrollmentView>('/prompts/enrollment'),
  /** Signed out is fine: the token in the link is the proof. */
  unsubscribe: (token: string) =>
    post<{ unsubscribed: boolean }>(`/prompts/unsubscribe?token=${encodeURIComponent(token)}`),

  /** The caller's own answer, or null when not started. */
  async entry(day: string): Promise<PromptEntryView | null> {
    try {
      return await get<PromptEntryView>(`/prompts/${day}/entry`)
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) return null
      throw e
    }
  },
  saveEntry: (day: string, content: DocNode, baseHash: string | null, localDay: string) =>
    put<PromptEntryView>(`/prompts/${day}/entry`, { content, baseHash, localDay }),
  submit: (day: string) => post<PromptEntryView>(`/prompts/${day}/entry/submit`),
  withdraw: (day: string) => post<PromptEntryView>(`/prompts/${day}/entry/withdraw`),
  deleteEntry: (day: string) => del<{ deleted: boolean }>(`/prompts/${day}/entry`),

  feed: (day: string, sort: 'newest' | 'fewest_critiques', cursor?: string | null) =>
    get<EntryPage>(
      `/prompts/${day}/entries?sort=${sort}${cursor ? `&cursor=${encodeURIComponent(cursor)}` : ''}`,
    ),
  get: (entryId: string) => get<PromptEntryView>(`/prompt-entries/${entryId}`),
  previewWork: (entryId: string) => post<ToWorkPreview>(`/prompt-entries/${entryId}/to-work/preview`),
  toWork: (entryId: string, title: string) => post<Outline>(`/prompt-entries/${entryId}/to-work`, { title }),
}

export const critiques = {
  list: (entryId: string) => get<CritiqueView[]>(`/prompt-entries/${entryId}/critiques`),
  create: (entryId: string, body: string, parentId?: string | null) =>
    post<CritiqueView>(`/prompt-entries/${entryId}/critiques`, { body, parentId: parentId ?? null }),
  update: (id: string, body: string) => patch<CritiqueView>(`/prompt-critiques/${id}`, { body }),
  remove: (id: string) => del<{ removed: boolean }>(`/prompt-critiques/${id}`),
  report: (id: string, reason: ReportReason, detail?: string) =>
    post<{ reported: boolean }>(`/prompt-critiques/${id}/report`, { reason, detail: detail || null }),
  reports: (entryId: string) => get<CritiqueReportView[]>(`/prompt-entries/${entryId}/reports`),
}

export const inbox = {
  list: (cursor?: string | null) =>
    get<InboxPage>(`/inbox${cursor ? `?cursor=${encodeURIComponent(cursor)}` : ''}`),
  unread: () => get<UnreadCount>('/inbox/unread-count'),
  read: (id: string) => post<UnreadCount>(`/inbox/${id}/read`),
  readDigest: (day: string) => post<UnreadCount>(`/inbox/digests/${day}/read`),
  readAll: () => post<UnreadCount>('/inbox/read-all'),
}

const qs = (params: Record<string, string | number | boolean | null | undefined>) => {
  const q = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== '') q.set(k, String(v))
  const text = q.toString()
  return text ? `?${text}` : ''
}

/** What a restriction form sends. */
export interface RestrictInput {
  kind: 'warning' | 'ban' | 'suspend' | 'read_only'
  scope: 'site' | 'discussion'
  /** 3, 7 or 30; absent for permanent. */
  days?: number | null
  reasonCode: string
  reasonText: string
  internalNote?: string
  itemIds?: string[]
  confirmName?: string
}

export const staff = {
  overview: () => get<Overview>('/admin/overview'),
  users: (params: { q?: string; status?: string; role?: string; verified?: string; sort?: string; cursor?: string }) =>
    get<UserList>(`/admin/users${qs(params)}`),
  user: (id: string) => get<UserDetail>(`/admin/users/${id}`),
  createUser: (input: { email: string; displayName: string; moderator?: boolean }) =>
    post<{ id: string }>('/admin/users', input),
  editUser: (id: string, input: {
    displayName?: string; bio?: string; aboutAuthor?: string; email?: string
    markVerified?: boolean; resetTwoFactor?: boolean; reason: string
  }) => patch<UserDetail>(`/admin/users/${id}`, input),
  deleteUser: (id: string, input: { confirmName: string; reason: string; deleteSharedWorks?: boolean; skipExport?: boolean }) =>
    request<{ worksDeleted: number }>('DELETE', `/admin/users/${id}`, input),
  addNote: (id: string, body: string) => post<NoteView>(`/admin/users/${id}/notes`, { body }),
  setModerator: (id: string, moderator: boolean, reason: string) =>
    post<{ ok: boolean }>(`/admin/users/${id}/moderator`, { moderator, reason }),
  sendReset: (id: string, signOut: boolean) => post<{ ok: boolean }>(`/admin/users/${id}/password-reset`, { signOut }),
  resendSetup: (id: string) => post<{ ok: boolean }>(`/admin/users/${id}/setup-email`),
  revokeAll: (id: string) => post<{ ok: boolean }>(`/admin/users/${id}/sessions/revoke-all`),
  revokeOne: (id: string, sessionId: string) => del<{ ok: boolean }>(`/admin/users/${id}/sessions/${sessionId}`),
  deactivate: (id: string, reason: string) => post<{ ok: boolean }>(`/admin/users/${id}/deactivate`, { reason }),
  reactivate: (id: string, reason: string) => post<{ ok: boolean }>(`/admin/users/${id}/reactivate`, { reason }),
  deleteContent: (id: string, reason: string) => post<{ removed: number }>(`/admin/users/${id}/delete-content`, { reason }),
  restrict: (id: string, input: RestrictInput) => post<RestrictionView>(`/admin/users/${id}/restrictions`, input),
  changeRestriction: (id: string, input: { days?: number | null; reasonText?: string; reason: string }) =>
    patch<RestrictionView>(`/admin/restrictions/${id}`, input),
  lift: (id: string, reason: string) => post<RestrictionView>(`/admin/restrictions/${id}/lift`, { reason }),
  appeals: (state: 'open' | 'all') => get<AppealRow[]>(`/admin/appeals${qs({ state })}`),
  decideAppeal: (id: string, input: { decision: 'upheld' | 'overturned'; message: string; restoreContent?: boolean }) =>
    post<Appeal>(`/admin/appeals/${id}/decide`, input),
  bans: (params: { q?: string; includeRemoved?: boolean }) => get<PermanentBan[]>(`/admin/permanent-bans${qs(params)}`),
  addBan: (input: { email: string; displayName?: string; reasonCode: string; reasonText: string }) =>
    post<PermanentBan>('/admin/permanent-bans', input),
  removeBan: (id: string, reason: string) => post<PermanentBan>(`/admin/permanent-bans/${id}/remove`, { reason }),
  domains: () => get<BlockedDomain[]>('/admin/blocked-domains'),
  addDomain: (domain: string, note: string) => post<BlockedDomain>('/admin/blocked-domains', { domain, note }),
  removeDomain: (domain: string) => del<{ ok: boolean }>(`/admin/blocked-domains/${encodeURIComponent(domain)}`),
  queue: (params: { tab?: string; source?: string; reason?: string; surface?: string; mine?: boolean; offset?: number }) =>
    get<QueuePage>(`/admin/queue${qs(params)}`),
  claim: (id: string) => post<QueueItem>(`/admin/queue/${id}/claim`),
  decide: (id: string, action: 'dismiss' | 'remove' | 'restore' | 'hold' | 'release', reason: string) =>
    post<QueueItem>(`/admin/queue/${id}/${action}`, { reason }),
  bulkDismiss: (itemIds: string[], source: string, reason: string) =>
    post<{ dismissed: number }>('/admin/queue/dismiss', { itemIds, source, reason }),
  words: () => get<WordList>('/admin/words'),
  addTerm: (input: { term: string; matchKind: string; action: string; appliesTo: string[]; note?: string }) =>
    post<WordFilterTerm>('/admin/words', input),
  updateTerm: (id: string, input: { term?: string; matchKind?: string; action?: string; appliesTo?: string[]; note?: string }) =>
    patch<WordFilterTerm>(`/admin/words/${id}`, input),
  deleteTerm: (id: string) => del<{ ok: boolean }>(`/admin/words/${id}`),
  testWords: (text: string, surface?: string) => post<TestResult>('/admin/words/test', { text, surface }),
  importWords: (csv: string) => post<{ added: number }>('/admin/words/import', { csv }),
  allowWord: (word: string) => post<{ ok: boolean }>('/admin/words/allow', { word }),
  unallowWord: (word: string) => del<{ ok: boolean }>(`/admin/words/allow/${encodeURIComponent(word)}`),
  rescan: () => post<{ checked: number; flagged: number }>('/admin/words/rescan'),
  /** The list as CSV text. */
  async exportWords(): Promise<string> {
    const res = await fetch(`${BASE}/admin/words/export`, { headers: { authorization: `Bearer ${tokens.access() ?? ''}` } })
    if (!res.ok) throw new ApiError(res.status, 'unknown', res.statusText)
    return res.text()
  },
  external: () => get<ExternalView>('/admin/external'),
  saveExternal: (input: {
    provider: string; baseUrl?: string; surfaces: string[]; removeAs: string; banScope: string
    apiKey?: string; webhookSecret?: string
  }) => put<ExternalView>('/admin/external', input),
  testExternal: (text: string, surface?: string) => post<ExternalTestResult>('/admin/external/test', { text, surface }),
  settings: () => get<ModerationSettings>('/admin/moderation-settings'),
  saveSettings: (input: ModerationSettings) => put<ModerationSettings>('/admin/moderation-settings', input),
  log: (params: { actor?: string; subject?: string; action?: string; from?: string; to?: string; cursor?: string }) =>
    get<LogPage>(`/admin/log${qs(params)}`),
  /** The log as CSV text, for administrators. */
  async logCsv(): Promise<string> {
    const res = await fetch(`${BASE}/admin/log.csv`, { headers: { authorization: `Bearer ${tokens.access() ?? ''}` } })
    if (!res.ok) throw new ApiError(res.status, 'unknown', res.statusText)
    return res.text()
  },
}

/** Where the signed-in writer stands, and what they can do about it. */
export const standing = {
  mine: () => get<AccountStanding>('/me/restrictions'),
  appeal: (restrictionId: string, body: string) =>
    post<MyAppeal>(`/me/restrictions/${restrictionId}/appeal`, { body }),
  /** The restriction an emailed link is for — no sign-in needed. */
  peek: (token: string) => get<MyRestriction>(`/ban/appeal/${encodeURIComponent(token)}`),
  appealFromLink: (token: string, body: string) => post<MyAppeal>('/ban/appeal', { token, body }),
  emailMyWork: (email: string) => post<{ ok: boolean }>('/ban/export', { email }),
  /** Choosing a password from the link a staff-made account was sent. */
  setup: (token: string, password: string, acceptTerms: boolean) =>
    request<Session>('POST', '/auth/setup', { token, password, acceptTerms }),
  report: (input: { subjectType: string; subjectId: string; reason: ReportReason; detail?: string }) =>
    post<Reported>('/reports', input),
}

export const admin = {
  prompts: () => get<AdminPrompts>('/admin/prompts'),
  addPrompt: (body: string, day?: string | null) => post<AdminPrompts>('/admin/prompts', { body, day: day || null }),
  updatePrompt: (id: string, input: { body?: string; day?: string | null; unpin?: boolean }) =>
    patch<AdminPrompts>(`/admin/prompts/${id}`, input),
  deletePrompt: (id: string) => del<AdminPrompts>(`/admin/prompts/${id}`),
  reorder: (ids: string[]) => post<AdminPrompts>('/admin/prompts/reorder', { ids }),
  jobs: () => get<JobsReport>('/admin/jobs'),
}

export interface CommentFilters {
  sceneId?: string
  sort?: 'top' | 'new'
  category?: CommentCategory
  state?: 'open' | 'resolved'
  authorId?: string
  minScore?: number
}

/** How the review feed is narrowed and ordered. */
export interface FeedFilters {
  kind?: WorkKind
  genre?: Genre
  length?: LengthBand
  window?: WindowFilter
  sort?: FeedSort
}

// ── Export ──────────────────────────────────────────────────────────────────

/** A request with the access token, refreshed once if it has lapsed. */
async function authed(method: string, path: string, body?: BodyInit | null, type?: string): Promise<Response> {
  const send = () => {
    const headers: Record<string, string> = { 'x-request-id': newRequestId(), ...shareKeys.header() }
    if (type) headers['content-type'] = type
    const access = tokens.access()
    if (access) headers.authorization = `Bearer ${access}`
    return fetch(`${BASE}${path}`, { method, headers, body })
  }
  let res = await send()
  if (res.status === 401 && (await tryRefresh())) res = await send()
  if (!res.ok) {
    const payload = await res.json().catch(() => null)
    const error = payload?.error
    throw new ApiError(
      res.status,
      error?.code ?? 'unknown',
      error?.message ?? res.statusText,
      error?.details,
      error?.requestId ?? res.headers.get('x-request-id') ?? undefined,
    )
  }
  return res
}

const json = (value: unknown) => JSON.stringify(value)

/** Hands a response body to the browser as a download, under the name the server chose. */
async function saveResponse(res: Response, fallback: string): Promise<{ filename: string; warnings: number }> {
  const named = /filename="([^"]+)"/.exec(res.headers.get('content-disposition') ?? '')
  const filename = named?.[1] ?? fallback
  const url = URL.createObjectURL(await res.blob())
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  document.body.appendChild(link)
  link.click()
  link.remove()
  setTimeout(() => URL.revokeObjectURL(url), 10_000)
  return { filename, warnings: Number(res.headers.get('x-export-warnings') ?? 0) }
}

/** Where an export's settings go: a version is frozen, the working draft is as it stands. */
export type ExportTarget = { version: string } | { work: string }

const targetPath = (target: ExportTarget) =>
  'version' in target ? `/versions/${target.version}` : `/works/${target.work}`

export const exporting = {
  preflight: (target: ExportTarget, options: ExportOptions) =>
    request<PreflightReport>('POST', `${targetPath(target)}/export/preflight`, options),

  async download(target: ExportTarget, options: ExportOptions) {
    const res = await authed('POST', `${targetPath(target)}/export`, json(options), 'application/json')
    return saveResponse(res, `manuscript.${options.format ?? 'docx'}`)
  },

  async bundle(target: ExportTarget, formats: ExportFormat[], options: ExportOptions) {
    const res = await authed('POST', `${targetPath(target)}/export/bundle`, json({ formats, options }), 'application/json')
    return saveResponse(res, 'manuscript.zip')
  },

  /** Opens a preview in a new tab: the first pages of a PDF, or a web page. */
  async preview(target: ExportTarget, options: ExportOptions) {
    const res = await authed('POST', `${targetPath(target)}/export/preview`, json(options), 'application/json')
    const blob = await res.blob()
    const url = URL.createObjectURL(blob)
    window.open(url, '_blank', 'noopener')
    setTimeout(() => URL.revokeObjectURL(url), 60_000)
  },

  profiles: () => get<ExportProfiles>('/export/profiles'),
  saveProfile: (name: string, options: ExportOptions) =>
    post<ExportProfile>('/export/profiles', { name, options }),
  updateProfile: (id: string, input: { name?: string; options?: ExportOptions }) =>
    patch<ExportProfile>(`/export/profiles/${id}`, input),
  deleteProfile: (id: string) => del<void>(`/export/profiles/${id}`),
  /** From a file: the server repairs what has gone stale and lists it in `notes`. */
  importProfile: (name: string, options: unknown) =>
    post<ExportProfile>('/export/profiles/import', { name, options }),

  last: (workId: string) => get<LastUsedResponse>(`/works/${workId}/export/last`),
  async again(workId: string) {
    const res = await authed('POST', `/works/${workId}/export/again`, '{}', 'application/json')
    return saveResponse(res, 'manuscript')
  },

  history: (workId: string) => get<ExportHistoryItem[]>(`/works/${workId}/export/history`),
  async downloadAgain(workId: string, historyId: string) {
    const res = await authed('GET', `/works/${workId}/export/history/${historyId}/download`)
    const saved = await saveResponse(res, 'manuscript')
    return { ...saved, identical: res.headers.get('x-export-identical') === 'true' }
  },
}

/** What the book says about itself for export: language, description, cover. */
export const bookDetails = {
  get: (workId: string) => get<MetadataResponse>(`/works/${workId}/metadata`),
  save: (workId: string, input: UpdateMetadataRequest) => patch<MetadataResponse>(`/works/${workId}/metadata`, input),
  async setCover(workId: string, file: File) {
    const res = await authed('PUT', `/works/${workId}/cover`, file, file.type || 'application/octet-stream')
    return (await res.json()) as MetadataResponse
  },
  removeCover: (workId: string) => del<MetadataResponse>(`/works/${workId}/cover`),
  /** The cover as an object URL, since it is behind the token. The caller revokes it. */
  async coverUrl(workId: string): Promise<string> {
    const res = await authed('GET', `/works/${workId}/cover`)
    return URL.createObjectURL(await res.blob())
  },
}
