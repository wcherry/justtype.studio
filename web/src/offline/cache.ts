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

export const cache = {
  outline: (workId: string) => read<Outline>(OUTLINE_KEY(workId)),
  putOutline: (workId: string, outline: Outline) => write(OUTLINE_KEY(workId), outline),
  scene: (sceneId: string) => read<SceneWithContent>(SCENE_KEY(sceneId)),
  putScene: (scene: SceneWithContent) => write(SCENE_KEY(scene.id), scene),
}
