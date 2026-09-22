import type {
  Comment, CommentView, DiffResponse, DocNode, Outline, Publication, PublicationCard,
  RevisionWithContent, SaveResult, Scene, SceneRevision, SceneWithContent, Session, Template,
  TrackedChange,
  User, Version, VersionContent, VersionDetail, Work, WorkKind, Visibility, CommentCategory,
  WritingSummary,
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
export async function downloadExport(
  versionId: string,
  format: string,
  style: string,
): Promise<void> {
  const params = new URLSearchParams({ format, style })
  const headers: Record<string, string> = {}
  const access = tokens.access()
  if (access) headers.authorization = `Bearer ${access}`

  let res = await fetch(`${BASE}/versions/${versionId}/export?${params}`, { headers })
  if (res.status === 401 && (await tryRefresh())) {
    const retry = tokens.access()
    if (retry) headers.authorization = `Bearer ${retry}`
    res = await fetch(`${BASE}/versions/${versionId}/export?${params}`, { headers })
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
    update: (
      sceneId: string,
      input: Partial<Pick<Scene, 'title' | 'summary' | 'pov' | 'status' | 'chapterId'>> & {
        /** `null` clears the target; omitting leaves it alone. */
        targetWords?: number | null
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
    }) => post<TrackedChange>(`/scenes/${sceneId}/changes`, input),
    accept: (changeId: string) => post<TrackedChange>(`/changes/${changeId}/accept`),
    reject: (changeId: string) => post<TrackedChange>(`/changes/${changeId}/reject`),
  },

  versions: {
    list: (workId: string) => get<Version[]>(`/works/${workId}/versions`),
    create: (workId: string, label?: string, notes?: string) =>
      post<VersionDetail>(`/works/${workId}/versions`, { label, notes }),
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
    }) => post<PublicationCard>(`/versions/${versionId}/publications`, input),
    feed: (opts: { mine?: boolean; kind?: string } = {}) => {
      const params = new URLSearchParams()
      if (opts.mine) params.set('mine', 'true')
      if (opts.kind) params.set('kind', opts.kind)
      const qs = params.toString()
      return get<PublicationCard[]>(`/publications${qs ? `?${qs}` : ''}`)
    },
    get: (publicationId: string) => get<PublicationCard>(`/publications/${publicationId}`),
    content: (publicationId: string) => get<VersionContent>(`/publications/${publicationId}/content`),
    update: (publicationId: string, input: Partial<Pick<Publication, 'visibility' | 'reviewPrompt' | 'closesAt'>>) =>
      patch<PublicationCard>(`/publications/${publicationId}`, input),
    remove: (publicationId: string) => del<{ ok: boolean }>(`/publications/${publicationId}`),
  },

  comments: {
    list: (publicationId: string, opts: { sceneId?: string; sort?: 'top' | 'new' } = {}) => {
      const params = new URLSearchParams()
      if (opts.sceneId) params.set('sceneId', opts.sceneId)
      if (opts.sort) params.set('sort', opts.sort)
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
  },
}
