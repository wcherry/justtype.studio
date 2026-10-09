import type { Outline } from './api/types'

/*
 * A new chapter or scene is untitled. Wherever it is shown it is called by
 * where it falls — "Chapter 3", "Scene 2", counting scenes within their
 * chapter — so the name follows it when it moves, until the writer gives it
 * one. The server names them the same way when a version is taken.
 */

export const isUntitled = (title: string) => title.trim() === ''

export const chapterName = (title: string, number: number) => title.trim() || `Chapter ${number}`

export const sceneName = (title: string, number: number) => title.trim() || `Scene ${number}`

/** The outline with every untitled chapter and scene named, for showing — never for saving. */
export function withPlaceholderNames(outline: Outline): Outline {
  // Front and back matter are not numbered, and do not take a number from the
  // chapters around them: "Chapter 4" is the fourth chapter.
  let numbered = 0
  return {
    ...outline,
    chapters: outline.chapters.map((chapter) => ({
      ...chapter,
      title:
        chapter.role === 'chapter'
          ? chapterName(chapter.title, ++numbered)
          : chapter.title.trim() || (chapter.role === 'front' ? 'Front matter' : 'Back matter'),
      scenes: chapter.scenes.map((scene, s) => ({ ...scene, title: sceneName(scene.title, s + 1) })),
    })),
  }
}
