import { useEffect, useState } from 'react'
import type { Outline as OutlineType, Scene } from '../api/types'
import { chapterNames, isUntitled, roleBadge, sceneName, type ChapterRole } from '../naming'
import { ContextMenu, type MenuItem } from './Menu'

interface Props {
  outline: OutlineType
  activeSceneId: string | null
  onSelectScene: (scene: Scene) => void
  /** Adds an untitled chapter at the end, shown as "Chapter N" until named. */
  onAddChapter: () => void
  /** Adds an untitled scene at the end of a chapter, shown as "Scene N". */
  onAddScene: (chapterId: string) => void
  onMoveScene: (chapterId: string, sceneIds: string[]) => void
  onRenameScene: (sceneId: string, title: string) => void
  onRenameChapter?: (chapterId: string, title: string) => void
  /** Makes a chapter front matter, back matter, a part, or a chapter again. */
  onSetRole?: (chapterId: string, role: ChapterRole) => void
  /**
   * A scene dragged, or moved from the keyboard or its menu, to a place in
   * `toChapterId`, possibly another chapter: `ids` is that chapter's new order.
   */
  onMoveSceneTo?: (sceneId: string, fromChapterId: string, toChapterId: string, ids: string[]) => void
  /** The whole book's chapters in a new order, after a chapter or part is moved. */
  onReorderChapters?: (ids: string[]) => void
  /**
   * The actions on a scene, for its menu (right-click, Ctrl-click, or the menu
   * key). `move` moves it one place, across into the next chapter at an end.
   */
  sceneMenu?: (scene: Scene, chapterId: string, move: { up?: () => void; down?: () => void }) => MenuItem[]
  /** Whether this chapter's scenes may be added, moved or renamed. */
  canWrite?: (chapterId: string) => boolean
  /** Whether chapters may be added. */
  canShape?: boolean
  /** Added to the outline's own class, for where it is shown. */
  className?: string
}

/**
 * Which chapters and parts are folded away, per book and per device: a long
 * book is easier to find your way around with most of it closed, and how much
 * of it you want open depends on the screen. Storage can be missing or full;
 * the outline then simply starts open.
 */
const foldedKey = (workId: string) => `justtype.outlineFolded.${workId}`

function loadFolded(workId: string): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(foldedKey(workId)) ?? '[]')
    return new Set(Array.isArray(raw) ? raw.filter((id): id is string => typeof id === 'string') : [])
  } catch {
    return new Set()
  }
}

function saveFolded(workId: string, folded: Set<string>) {
  try {
    if (folded.size) localStorage.setItem(foldedKey(workId), JSON.stringify([...folded]))
    else localStorage.removeItem(foldedKey(workId))
  } catch {
    // Not kept; the outline still folds for as long as it is open.
  }
}

/**
 * For each chapter, the part it falls under, if any. A part holds the chapters
 * after it, up to the next part or the back matter.
 */
function partsOf(chapters: OutlineType['chapters']): (string | null)[] {
  let part: string | null = null
  return chapters.map((chapter) => {
    if (chapter.role === 'part') {
      part = chapter.id
      return null
    }
    if (chapter.role !== 'chapter') part = null
    return part
  })
}

type Chapters = OutlineType['chapters']

/** `block` taken out of `ids` and put back in before what is now at `at`. */
function moveBlock(ids: string[], block: string[], at: number): string[] {
  const rest = ids.filter((id) => !block.includes(id))
  // Indexes after the block's old place shift left once it is taken out.
  const before = ids.slice(0, at).filter((id) => !block.includes(id)).length
  return [...rest.slice(0, before), ...block, ...rest.slice(before)]
}

/** A chapter, or a part and the chapters it holds: what moves together. */
function blockOf(chapters: Chapters, owner: (string | null)[], index: number): string[] {
  const chapter = chapters[index]
  if (chapter.role !== 'part') return [chapter.id]
  return [chapter.id, ...chapters.filter((_, i) => owner[i] === chapter.id).map((c) => c.id)]
}

/** Where a drop lands in a list of rows: before the row, or after it. */
const lowerHalf = (e: React.DragEvent) => {
  const box = e.currentTarget.getBoundingClientRect()
  return e.clientY > box.top + box.height / 2
}

type Drag =
  | { kind: 'scene'; id: string; from: string }
  | { kind: 'chapter'; ids: string[] }

type Drop =
  | { kind: 'scene'; chapterId: string; index: number; marker: string; after: boolean }
  | { kind: 'chapter'; index: number; marker: string; after: boolean }

const STATUS_DOT: Record<Scene['status'], string> = {
  outline: '○',
  drafting: '◐',
  revised: '◑',
  done: '●',
}

export function Outline({
  outline, activeSceneId, onSelectScene, onAddChapter, onAddScene, onMoveScene, onRenameScene, onRenameChapter, onSetRole,
  onMoveSceneTo, onReorderChapters, sceneMenu,
  canWrite = () => true, canShape = true, className,
}: Props) {
  const [drag, setDrag] = useState<Drag | null>(null)
  const [drop, setDrop] = useState<Drop | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number; scene: Scene; chapterId: string } | null>(null)
  const endDrag = () => {
    setDrag(null)
    setDrop(null)
  }
  /** The one name being edited in place, and what has been typed into it. */
  const [editing, setEditing] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const edit = (id: string, shown: string) => {
    setEditing(id)
    setDraft(shown)
  }
  /** Commits on Enter or leaving; a name left as it was shown stays unchanged. */
  const renameField = (id: string, shown: string, current: string, save: (title: string) => void) => (
    <input
      className="outline__rename"
      autoFocus
      value={draft}
      aria-label="Name"
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === 'Escape') setEditing(null)
        else if (e.key === 'Enter') e.currentTarget.blur()
      }}
      onBlur={() => {
        if (editing !== id) return
        setEditing(null)
        const title = draft.trim()
        // Left as the default it was shown as, or emptied: still untitled.
        const next = title === shown ? current.trim() : title
        if (next !== current.trim()) save(next)
      }}
    />
  )

  /** A scene to `index` in `toChapter`: within its chapter, or into another. */
  const placeScene = (sceneId: string, from: string, toChapter: string, index: number) => {
    const target = outline.chapters.find((c) => c.id === toChapter)
    if (!target) return
    const ids = target.scenes.map((s) => s.id)
    const at = from === toChapter ? ids.indexOf(sceneId) : -1
    if (at >= 0) {
      ids.splice(at, 1)
      if (at < index) index -= 1
    }
    ids.splice(Math.max(0, Math.min(index, ids.length)), 0, sceneId)
    if (from === toChapter && ids.every((id, i) => id === target.scenes[i]?.id)) return
    if (onMoveSceneTo) onMoveSceneTo(sceneId, from, toChapter, ids)
    else if (from === toChapter) onMoveScene(toChapter, ids)
  }

  /**
   * One place up or down: within the chapter, or at either end into the
   * chapter before or after it that this writer may change.
   */
  const stepScene = (scene: Scene, chapterIndex: number, delta: -1 | 1): (() => void) | undefined => {
    const chapter = outline.chapters[chapterIndex]
    const at = chapter.scenes.findIndex((s) => s.id === scene.id)
    const within = at + delta
    if (within >= 0 && within < chapter.scenes.length) {
      return () => placeScene(scene.id, chapter.id, chapter.id, delta < 0 ? within : within + 1)
    }
    if (!onMoveSceneTo) return undefined
    for (let c = chapterIndex + delta; c >= 0 && c < outline.chapters.length; c += delta) {
      const next = outline.chapters[c]
      if (!canWrite(next.id)) continue
      return () => placeScene(scene.id, chapter.id, next.id, delta < 0 ? next.scenes.length : 0)
    }
    return undefined
  }

  /** A chapter (with its part's chapters, for a part) one place up or down. */
  const stepChapter = (index: number, delta: -1 | 1) => {
    if (!onReorderChapters) return
    const ids = outline.chapters.map((c) => c.id)
    const block = blockOf(outline.chapters, owner, index)
    const first = ids.indexOf(block[0])
    const last = ids.indexOf(block[block.length - 1])
    if (delta < 0) {
      if (first === 0) return
      // Over the chapter above; for a part, over the whole part above it.
      let to = first - 1
      if (outline.chapters[index].role === 'part' && owner[to]) to = ids.indexOf(owner[to] as string)
      onReorderChapters(moveBlock(ids, block, to))
    } else {
      if (last === ids.length - 1) return
      const nextBlock = blockOf(outline.chapters, owner, last + 1)
      const over = outline.chapters[index].role === 'part' || outline.chapters[last + 1].role === 'part'
        ? nextBlock.length
        : 1
      onReorderChapters(moveBlock(ids, block, last + 1 + over))
    }
  }

  /** Lands what is being dragged where the marker says. */
  const finishDrop = () => {
    if (drag && drop) {
      if (drag.kind === 'scene' && drop.kind === 'scene') {
        placeScene(drag.id, drag.from, drop.chapterId, drop.index)
      } else if (drag.kind === 'chapter' && drop.kind === 'chapter' && onReorderChapters) {
        const ids = outline.chapters.map((c) => c.id)
        const next = moveBlock(ids, drag.ids, drop.index)
        if (next.some((id, i) => id !== ids[i])) onReorderChapters(next)
      }
    }
    endDrag()
  }

  const openMenu = (e: React.MouseEvent, scene: Scene, chapterId: string) => {
    if (!sceneMenu) return
    e.preventDefault()
    // From the keyboard there is no pointer: open it by the row instead.
    let { clientX: x, clientY: y } = e
    if (x === 0 && y === 0) {
      const box = e.currentTarget.getBoundingClientRect()
      x = box.left + 24
      y = box.bottom
    }
    setMenu({ x, y, scene, chapterId })
  }

  const names = chapterNames(outline.chapters)
  const owner = partsOf(outline.chapters)

  const [folded, setFolded] = useState<Set<string>>(() => loadFolded(outline.id))
  useEffect(() => setFolded(loadFolded(outline.id)), [outline.id])
  const fold = (next: Set<string>) => {
    setFolded(next)
    saveFolded(outline.id, next)
  }
  const toggle = (chapterId: string) => {
    const next = new Set(folded)
    if (next.has(chapterId)) next.delete(chapterId)
    else next.add(chapterId)
    fold(next)
  }
  const foldable = outline.chapters.filter((c) => c.scenes.length > 0 || c.role === 'part')
  const allFolded = foldable.length > 0 && foldable.every((c) => folded.has(c.id))

  // Going to a scene somewhere else (a search, a link, the next scene) opens
  // the chapter and the part it is in, so the outline always shows where you
  // are. Folding the chapter you are in is still yours to do.
  useEffect(() => {
    if (!activeSceneId) return
    const c = outline.chapters.findIndex((chapter) => chapter.scenes.some((s) => s.id === activeSceneId))
    if (c < 0) return
    const around = [outline.chapters[c].id, owner[c]].filter((id): id is string => !!id && folded.has(id))
    if (around.length === 0) return
    const next = new Set(folded)
    around.forEach((id) => next.delete(id))
    fold(next)
    // Only on arriving at a scene, not on every fold.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeSceneId])

  return (
    <nav className={className ? `outline ${className}` : 'outline'}>
      <header className="outline__header">
        <h2>{outline.title}</h2>
        <p className="outline__meta">
          {outline.wordCount.toLocaleString()} words · {outline.status}
          {foldable.length > 1 && (
            <button
              type="button"
              className="outline__fold-all"
              onClick={() => fold(allFolded ? new Set() : new Set(foldable.map((c) => c.id)))}
            >
              {allFolded ? 'Expand all' : 'Collapse all'}
            </button>
          )}
        </p>
      </header>

      {outline.chapters.map((chapter, c) => {
        // A chapter in a folded part is not shown at all.
        const part = owner[c]
        if (part && folded.has(part)) return null
        const writable = canWrite(chapter.id)
        const shown = names[c]
        const badge = roleBadge(chapter.role)
        const isFolded = folded.has(chapter.id)
        const canFold = chapter.scenes.length > 0 || chapter.role === 'part'
        // What a folded part is hiding: its chapters, and their words.
        const held = chapter.role === 'part'
          ? outline.chapters.filter((_, i) => owner[i] === chapter.id)
          : []
        // As the book counts them: a scene left out of it is left out here.
        const words = [chapter, ...held].reduce(
          (sum, ch) => sum + ch.scenes.reduce((n, s) => n + (s.excluded ? 0 : s.wordCount), 0),
          0,
        )
        const movable = canShape && !!onReorderChapters && editing !== chapter.id
        const chapterDrop = drop?.kind === 'chapter' && drop.marker === chapter.id ? drop : null
        const intoDrop = drop?.kind === 'scene' && drop.marker === `into:${chapter.id}`
        const bodyId = `outline-${chapter.id}`
        return (
        <section
          key={chapter.id}
          className={[
            'outline__chapter',
            chapter.role === 'part' && 'outline__chapter--part',
            isFolded && 'is-folded',
            !writable && 'is-read-only',
            drag?.kind === 'chapter' && drag.ids.includes(chapter.id) && 'is-dragging',
            chapterDrop && (chapterDrop.after ? 'is-drop-after' : 'is-drop-before'),
          ].filter(Boolean).join(' ')}
          onDragOver={(e) => {
            if (drag?.kind !== 'chapter' || drag.ids.includes(chapter.id)) return
            e.preventDefault()
            e.dataTransfer.dropEffect = 'move'
            const after = lowerHalf(e)
            // After a folded part means after everything in it.
            const end = isFolded && chapter.role === 'part' ? c + held.length + 1 : c + 1
            const index = after ? end : c
            if (drop?.kind !== 'chapter' || drop.index !== index || drop.marker !== chapter.id) {
              setDrop({ kind: 'chapter', index, marker: chapter.id, after })
            }
          }}
          onDrop={(e) => {
            if (drag?.kind !== 'chapter') return
            e.preventDefault()
            finishDrop()
          }}
        >
          <h3
            className={intoDrop ? 'is-drop-into' : undefined}
            draggable={movable}
            title={movable ? 'Drag to move' + (chapter.role === 'part' ? ' this part and its chapters' : '') : undefined}
            onDragStart={movable ? (e) => {
              if ((e.target as HTMLElement).closest('select, input')) {
                e.preventDefault()
                return
              }
              e.dataTransfer.effectAllowed = 'move'
              e.dataTransfer.setData('text/plain', chapter.id)
              setDrag({ kind: 'chapter', ids: blockOf(outline.chapters, owner, c) })
            } : undefined}
            onDragEnd={movable ? endDrag : undefined}
            onKeyDown={movable ? (e) => {
              if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
                e.preventDefault()
                stepChapter(c, e.key === 'ArrowUp' ? -1 : 1)
              }
            } : undefined}
            // A scene dropped on a chapter's heading goes to its end: the way
            // into a folded or empty chapter.
            onDragOver={(e) => {
              if (drag?.kind !== 'scene' || !writable || !onMoveSceneTo && drag.from !== chapter.id) return
              e.preventDefault()
              e.stopPropagation()
              if (!intoDrop) {
                setDrop({ kind: 'scene', chapterId: chapter.id, index: chapter.scenes.length, marker: `into:${chapter.id}`, after: true })
              }
            }}
            onDrop={(e) => {
              if (drag?.kind !== 'scene') return
              e.preventDefault()
              e.stopPropagation()
              finishDrop()
            }}
          >
            {canFold ? (
              <button
                type="button"
                className="outline__fold"
                aria-expanded={!isFolded}
                aria-controls={bodyId}
                aria-label={`${isFolded ? 'Expand' : 'Collapse'} ${shown}`}
                title={isFolded ? 'Expand' : chapter.role === 'part' ? 'Collapse this part and its chapters' : 'Collapse'}
                onClick={() => toggle(chapter.id)}
              >
                <span aria-hidden="true">{isFolded ? '▸' : '▾'}</span>
              </button>
            ) : (
              <span className="outline__fold outline__fold--none" aria-hidden="true" />
            )}
            {editing === chapter.id ? (
              renameField(chapter.id, shown, chapter.title, (t) => onRenameChapter?.(chapter.id, t))
            ) : (
              <span
                className={isUntitled(chapter.title) ? 'is-placeholder' : undefined}
                title={writable && onRenameChapter ? 'Double-click to name it' : undefined}
                onDoubleClick={() => writable && onRenameChapter && edit(chapter.id, shown)}
              >
                {shown}
              </span>
            )}
            {badge && (
              <span
                className="outline__badge"
                title={
                  chapter.role === 'part'
                    ? 'A divider before the chapters that follow, on a page of its own; not numbered as a chapter'
                    : 'Not numbered; included in an export with the front or back matter'
                }
              >
                {badge}
              </span>
            )}
            {writable && onSetRole && editing !== chapter.id && (
              <select
                className="outline__role"
                aria-label={`Role of ${shown}`}
                title="Chapter, part, front matter or back matter"
                value={chapter.role}
                onChange={(e) => onSetRole(chapter.id, e.target.value as ChapterRole)}
              >
                <option value="chapter">Chapter</option>
                <option value="part">Part</option>
                <option value="front">Front matter</option>
                <option value="back">Back matter</option>
              </select>
            )}
            {!writable && <span className="outline__lock" title="You can read this chapter but not change it"> · read only</span>}
            {isFolded && (
              <span className="outline__folded-count">
                {chapter.role === 'part'
                  ? `${held.length} ${held.length === 1 ? 'chapter' : 'chapters'} · ${words.toLocaleString()}`
                  : words.toLocaleString()}
              </span>
            )}
          </h3>
          {!isFolded && (
          <div id={bodyId}>
          <ul>
            {chapter.scenes.map((scene, index) => {
              const name = sceneName(scene.title, index + 1, chapter.role)
              const draggable = writable && editing !== scene.id
              const marked = drop?.kind === 'scene' && drop.marker === scene.id && drag?.kind === 'scene' && drag.id !== scene.id
              return (
              <li
                key={scene.id}
                className={[
                  'outline__scene',
                  scene.id === activeSceneId && 'is-active',
                  scene.excluded && 'is-excluded',
                  drag?.kind === 'scene' && drag.id === scene.id && 'is-dragging',
                  marked && (drop.after ? 'is-drop-after' : 'is-drop-before'),
                ].filter(Boolean).join(' ')}
                onDragOver={(e) => {
                  if (drag?.kind !== 'scene' || !writable) return
                  if (drag.from !== chapter.id && !onMoveSceneTo) return
                  e.preventDefault()
                  e.stopPropagation()
                  e.dataTransfer.dropEffect = 'move'
                  const after = lowerHalf(e)
                  const at = after ? index + 1 : index
                  if (drop?.kind !== 'scene' || drop.marker !== scene.id || drop.after !== after) {
                    setDrop({ kind: 'scene', chapterId: chapter.id, index: at, marker: scene.id, after })
                  }
                }}
                onDrop={(e) => {
                  if (drag?.kind !== 'scene') return
                  e.preventDefault()
                  e.stopPropagation()
                  finishDrop()
                }}
              >
                {editing === scene.id ? (
                  renameField(scene.id, name, scene.title, (t) => onRenameScene(scene.id, t))
                ) : (
                // A row rather than a button, so it can be dragged everywhere:
                // click to open, drag to move, right- or Ctrl-click for more.
                <div
                  role="button"
                  tabIndex={0}
                  className="outline__scene-btn"
                  aria-current={scene.id === activeSceneId ? 'true' : undefined}
                  aria-keyshortcuts={writable ? 'Alt+ArrowUp Alt+ArrowDown Shift+F10' : undefined}
                  draggable={draggable}
                  onClick={(e) => {
                    // Ctrl-click opens the menu everywhere; on a Mac the
                    // browser sends it as a right-click, which lands below.
                    if (e.ctrlKey && sceneMenu) openMenu(e, scene, chapter.id)
                    else onSelectScene(scene)
                  }}
                  onContextMenu={(e) => openMenu(e, scene, chapter.id)}
                  onKeyDown={(e) => {
                    if (e.target !== e.currentTarget) return
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault()
                      onSelectScene(scene)
                    } else if (writable && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
                      e.preventDefault()
                      stepScene(scene, c, e.key === 'ArrowUp' ? -1 : 1)?.()
                    }
                  }}
                  onDragStart={draggable ? (e) => {
                    e.dataTransfer.effectAllowed = 'move'
                    e.dataTransfer.setData('text/plain', scene.id)
                    e.stopPropagation()
                    setDrag({ kind: 'scene', id: scene.id, from: chapter.id })
                  } : undefined}
                  onDragEnd={draggable ? endDrag : undefined}
                >
                  <span className="outline__dot" title={scene.status}>{STATUS_DOT[scene.status]}</span>
                  <span
                    className={isUntitled(scene.title) ? 'outline__title is-placeholder' : 'outline__title'}
                    title={writable ? 'Double-click to name it' : undefined}
                    onDoubleClick={() => writable && edit(scene.id, name)}
                  >
                    {name}
                  </span>
                  {scene.excluded && (
                    <span className="outline__left-out" title="Not part of the book: not exported, not in a version, not counted">
                      not in book
                    </span>
                  )}
                  <span className="outline__words">{scene.wordCount}</span>
                </div>
                )}
              </li>
              )
            })}
          </ul>

          {writable && (
            <button
              type="button"
              className="outline__add-btn"
              onClick={() => onAddScene(chapter.id)}
            >
              + Scene
            </button>
          )}
          </div>
          )}
        </section>
        )
      })}

      {menu && sceneMenu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`Actions for ${menu.scene.title || 'this scene'}`}
          items={(() => {
            const c = outline.chapters.findIndex((ch) => ch.id === menu.chapterId)
            const scene = outline.chapters[c]?.scenes.find((s) => s.id === menu.scene.id) ?? menu.scene
            return sceneMenu(scene, menu.chapterId, c < 0 || !canWrite(menu.chapterId) ? {} : {
              up: stepScene(scene, c, -1),
              down: stepScene(scene, c, 1),
            })
          })()}
          onClose={() => setMenu(null)}
        />
      )}

      {canShape && (
      <button
        type="button"
        className="outline__add-btn outline__add-btn--chapter"
        onClick={() => onAddChapter()}
      >
        + Chapter
      </button>
      )}
    </nav>
  )
}

