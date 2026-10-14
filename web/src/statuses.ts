import type { Outline, StatusOption } from './api/types'

/**
 * The statuses every book has, as the server lists them: what an outline
 * cached before a book could have its own falls back to.
 */
export const BUILT_IN_STATUSES: StatusOption[] = [
  { key: 'outline', label: 'Outline', color: '#8e8e93', custom: false },
  { key: 'drafting', label: 'Drafting', color: '#d4882b', custom: false },
  { key: 'revised', label: 'Revised', color: '#4a7fc1', custom: false },
  { key: 'done', label: 'Done', color: '#3d9a5f', custom: false },
]

/** A book's statuses: its own first, then the built-in ones. */
export const statusesOf = (outline: Pick<Outline, 'statuses'>): StatusOption[] =>
  outline.statuses?.length ? outline.statuses : BUILT_IN_STATUSES

/** The status a scene has, or a plain grey stand-in for one since removed. */
export function statusOf(list: StatusOption[], key: string): StatusOption {
  return list.find((s) => s.key === key) ?? { key, label: key, color: '#8e8e93', custom: false }
}

/**
 * The dot before a scene's title. The built-in statuses fill as a scene
 * moves through them; the book's own are a full dot in their colour.
 */
const GLYPHS: Record<string, string> = { outline: '○', drafting: '◐', revised: '◑', done: '●' }
export const statusGlyph = (status: StatusOption) => (status.custom ? '●' : GLYPHS[status.key] ?? '●')
