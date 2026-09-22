/** Mirrors of the Rust API types. Kept by hand for now; see ROADMAP phase 0
 *  for generating these from an OpenAPI document instead. */

export type WorkKind = 'novel' | 'novella' | 'short_story' | 'serial'
export type WorkStatus = 'draft' | 'published' | 'final' | 'archived'
export type VersionState = 'draft' | 'published' | 'final'
export type SceneStatus = 'outline' | 'drafting' | 'revised' | 'done'
export type Visibility = 'unlisted' | 'platform' | 'public'
export type CommentCategory =
  | 'general' | 'praise' | 'plot' | 'pacing' | 'prose' | 'character' | 'typo'

/** A TipTap/ProseMirror document. Deliberately loose: the editor owns the schema. */
export interface DocNode {
  type: string
  content?: DocNode[]
  text?: string
  marks?: { type: string; attrs?: Record<string, unknown> }[]
  attrs?: Record<string, unknown>
}

export interface User {
  id: string
  email: string
  displayName: string
  bio: string | null
  createdAt: string
  updatedAt: string
}

export interface Session {
  user: User
  accessToken: string
  refreshToken: string
  expiresIn: number
}

export interface Template {
  id: string
  name: string
  kind: string
  description: string
  builtIn: boolean
  structure: { chapters: { title: string; scenes: { title: string }[] }[] }
}

export interface Work {
  id: string
  ownerId: string
  title: string
  subtitle: string | null
  kind: WorkKind
  synopsis: string | null
  templateId: string | null
  status: WorkStatus
  currentVersionId: string | null
  wordCount: number
  createdAt: string
  updatedAt: string
}

export interface Chapter {
  id: string
  workId: string
  title: string
  summary: string | null
  position: number
  createdAt: string
  updatedAt: string
}

export interface Scene {
  id: string
  workId: string
  chapterId: string
  title: string
  summary: string | null
  pov: string | null
  status: SceneStatus
  position: number
  wordCount: number
  contentHash: string
  createdAt: string
  updatedAt: string
}

export interface Outline extends Work {
  chapters: (Chapter & { scenes: Scene[] })[]
}

export interface SceneWithContent extends Scene {
  content: DocNode
}

export interface SaveResult {
  scene: Scene
  revisionId: string | null
  workWordCount: number
}

export interface SceneRevision {
  id: string
  sceneId: string
  workId: string
  authorId: string
  contentHash: string
  wordCount: number
  createdAt: string
}

export interface TrackedChange {
  id: string
  sceneId: string
  workId: string
  authorId: string
  kind: 'insert' | 'delete' | 'replace' | 'format'
  anchorFrom: number
  anchorTo: number
  quote: string | null
  payload: string
  note: string | null
  state: 'pending' | 'accepted' | 'rejected'
  resolvedBy: string | null
  resolvedAt: string | null
  createdAt: string
}

export interface Version {
  id: string
  workId: string
  number: number
  label: string
  state: VersionState
  parentVersionId: string | null
  createdBy: string
  notes: string | null
  wordCount: number
  createdAt: string
  publishedAt: string | null
  finalizedAt: string | null
}

export interface VersionEntry {
  id: string
  versionId: string
  chapterId: string
  chapterTitle: string
  chapterPosition: number
  sceneId: string
  sceneTitle: string
  scenePosition: number
  contentHash: string
  wordCount: number
}

export interface VersionDetail extends Version {
  entries: VersionEntry[]
}

export interface VersionContent extends Version {
  chapters: {
    chapterId: string
    title: string
    position: number
    scenes: {
      sceneId: string
      title: string
      position: number
      wordCount: number
      content: DocNode
    }[]
  }[]
}

export interface TextChange {
  op: 'equal' | 'insert' | 'delete'
  value: string
}

export interface SceneDiff {
  sceneId: string
  sceneTitle: string
  chapterTitle: string
  kind: 'added' | 'removed' | 'modified' | 'moved' | 'unchanged'
  wordsBefore: number
  wordsAfter: number
  changes?: TextChange[]
}

export interface DiffResponse {
  from: Version
  to: Version
  summary: {
    scenesAdded: number
    scenesRemoved: number
    scenesModified: number
    scenesMoved: number
    wordsAdded: number
    wordsRemoved: number
  }
  scenes: SceneDiff[]
}

export interface AuthorCard {
  id: string
  displayName: string
}

export interface Publication {
  id: string
  workId: string
  versionId: string
  visibility: Visibility
  scope: 'work' | 'chapter' | 'scene'
  scopeRefId: string | null
  reviewPrompt: string | null
  opensAt: string | null
  closesAt: string | null
  createdAt: string
  updatedAt: string
}

export interface PublicationCard extends Publication {
  workTitle: string
  workKind: WorkKind
  author: AuthorCard
  versionLabel: string
  wordCount: number
  commentCount: number
  excerpt: string
}

export interface Comment {
  id: string
  publicationId: string
  versionId: string
  sceneId: string | null
  parentId: string | null
  authorId: string
  body: string
  anchor: string | null
  anchorQuote: string | null
  category: CommentCategory
  state: 'open' | 'resolved' | 'deleted'
  score: number
  createdAt: string
  updatedAt: string
}

export interface CommentView extends Comment {
  author: AuthorCard
  viewerVote: number
  replies: CommentView[]
}

/** Parsed form of `Comment.anchor`. */
export interface Anchor {
  from: number
  to: number
}

export function parseAnchor(raw: string | null): Anchor | null {
  if (!raw) return null
  try {
    const parsed = JSON.parse(raw) as Anchor
    return typeof parsed.from === 'number' && typeof parsed.to === 'number' ? parsed : null
  } catch {
    return null
  }
}
