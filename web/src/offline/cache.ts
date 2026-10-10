import type { Outline, SceneWithContent } from '../api/types'

/**
 * A read cache for the structure and text the editor needs to open.
 *
 * The write queue keeps unsynced edits safe, but safe is not the same as
 * reachable: without this, reopening a work offline shows a load error and the
 * writer cannot get back to their own queued words. localStorage rather than
 * IndexedDB because these are small, whole-object reads on the render path.
 *
 * This is a cache, never a source of truth. A successful fetch always wins and
 * always refreshes it.
 */

const OUTLINE_KEY = (workId: string) => `justtype.outline.${workId}`
const SCENE_KEY = (sceneId: string) => `justtype.scene.${sceneId}`
/** Whose books the cache holds. Several writers can share one browser. */
const OWNER_KEY = 'justtype.cacheOwner'
/**
 * The text one writer's books leave in this browser: the cache, and their
 * dictionary (which is full of their characters' names). Where they left off
 * in each book is kept per writer instead, and survives this; see EditorPage.
 */
const PER_BOOK = ['justtype.outline.', 'justtype.scene.', 'justtype.dictionary']

function read<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : null
  } catch {
    return null
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // Quota exceeded or site data blocked. The cache is an optimisation; the
    // queue is what actually protects the writing, and it is elsewhere.
  }
}

function clear() {
  try {
    const keys: string[] = []
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i)
      if (key && PER_BOOK.some((prefix) => key.startsWith(prefix))) keys.push(key)
    }
    for (const key of keys) localStorage.removeItem(key)
    localStorage.removeItem(OWNER_KEY)
  } catch {
    // Blocked site data: there was nothing cached to leak either.
  }
}

export const cache = {
  /**
   * Hands the cache to whoever just signed in. Another writer's books are
   * dropped first: they are that writer's text, and an editor that falls back
   * to the cache would otherwise show them to the next person at the keyboard.
   */
  claim(userId: string) {
    try {
      if (localStorage.getItem(OWNER_KEY) === userId) return
    } catch {
      return
    }
    clear()
    try {
      localStorage.setItem(OWNER_KEY, userId)
    } catch {
      // Nothing was cached for anyone, so there is nothing to protect.
    }
  },
  /** On signing out: a shared machine should not keep the books readable. */
  clear,
  outline: (workId: string) => read<Outline>(OUTLINE_KEY(workId)),
  putOutline: (workId: string, outline: Outline) => write(OUTLINE_KEY(workId), outline),
  scene: (sceneId: string) => read<SceneWithContent>(SCENE_KEY(sceneId)),
  putScene: (scene: SceneWithContent) => write(SCENE_KEY(scene.id), scene),
}
