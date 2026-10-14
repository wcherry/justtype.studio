import type { Outline } from './api/types'

/*
 * A new chapter or scene is untitled. Wherever it is shown it is called by
 * where it falls — "Chapter 3", "Scene 2", counting scenes within their
 * chapter — so the name follows it when it moves, until the writer gives it
 * one. The server names them the same way when a version is taken.
 */

/** Where a chapter sits: a chapter, front or back matter, or a part that opens the chapters after it. */
export type ChapterRole = 'chapter' | 'front' | 'back' | 'part'

export const isUntitled = (title: string) => title.trim() === ''

export const chapterName = (title: string, number: number) => title.trim() || `Chapter ${number}`

export const partName = (title: string, number: number) => title.trim() || `Part ${number}`

/** `title`, or "Scene 2" for where it sits in its chapter; in a part, its "Part page". */
export function sceneName(title: string, number: number, role?: string): string {
  if (title.trim()) return title.trim()
  if (role === 'part') return number === 1 ? 'Part page' : `Part page ${number}`
  return `Scene ${number}`
}

/** What a chapter that is not numbered as one is, for a badge; nothing for a chapter. */
export function roleBadge(role: string): string | null {
  if (role === 'front') return 'front matter'
  if (role === 'back') return 'back matter'
  if (role === 'part') return 'part'
  return null
}

/**
 * Each chapter's name, in order. Front and back matter are not numbered, and do
 * not take a number from the chapters around them: "Chapter 4" is the fourth
 * chapter. Parts are counted on their own, and chapter numbers run on through
 * them.
 */
export function chapterNames(chapters: { title: string; role: string }[]): string[] {
  let chapterCount = 0
  let partCount = 0
  return chapters.map((chapter) => {
    if (chapter.role === 'part') return partName(chapter.title, ++partCount)
    if (chapter.role === 'front') return chapter.title.trim() || 'Front matter'
    if (chapter.role === 'back') return chapter.title.trim() || 'Back matter'
    return chapterName(chapter.title, ++chapterCount)
  })
}

/** The outline with every untitled chapter and scene named, for showing — never for saving. */
export function withPlaceholderNames(outline: Outline): Outline {
  const names = chapterNames(outline.chapters)
  return {
    ...outline,
    chapters: outline.chapters.map((chapter, c) => ({
      ...chapter,
      title: names[c],
      scenes: chapter.scenes.map((scene, s) => ({ ...scene, title: sceneName(scene.title, s + 1, chapter.role) })),
    })),
  }
}
