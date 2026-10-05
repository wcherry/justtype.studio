import type { WorkAccess } from '../api/types'

/**
 * What the caller may do with a work, read off the outline.
 *
 * The server decides; this only keeps the screen from offering what it would
 * refuse. An outline cached before collaboration existed carries no access,
 * and every work then had exactly one person on it — its owner.
 */
export interface Permissions {
  role: WorkAccess['role']
  /** Change the text and structure of this chapter. */
  canWrite: (chapterId: string) => boolean
  /** Propose changes and keep the to-do list for this chapter. */
  canSuggest: (chapterId: string) => boolean
  /** Add, order or remove chapters. */
  canShape: boolean
  /** Snapshots, publications, front matter, who is on the book. */
  isOwner: boolean
  /** Started it — the only one who can delete it. */
  isCreator: boolean
}

const RANK: Record<WorkAccess['role'], number> = { reader: 0, editor: 1, writer: 2, owner: 3 }

export function permissions(access: WorkAccess | null | undefined): Permissions {
  const role = access?.role ?? 'owner'
  const chapters = access?.chapterIds ?? null
  const within = (chapterId: string) => chapters === null || chapters.includes(chapterId)
  return {
    role,
    canWrite: (chapterId) => RANK[role] >= RANK.writer && within(chapterId),
    canSuggest: (chapterId) => RANK[role] >= RANK.editor && within(chapterId),
    canShape: RANK[role] >= RANK.writer && chapters === null,
    isOwner: role === 'owner',
    isCreator: access?.creator ?? true,
  }
}
