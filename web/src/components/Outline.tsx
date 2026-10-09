import { useState } from 'react'
import type { Outline as OutlineType, Scene } from '../api/types'
import { chapterName, isUntitled, sceneName } from '../naming'

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
  /** Makes a chapter front matter, back matter, or a chapter again. */
  onSetRole?: (chapterId: string, role: 'chapter' | 'front' | 'back') => void
  /** Whether this chapter's scenes may be added, moved or renamed. */
  canWrite?: (chapterId: string) => boolean
  /** Whether chapters may be added. */
  canShape?: boolean
  /** Added to the outline's own class, for where it is shown. */
  className?: string
}

const STATUS_DOT: Record<Scene['status'], string> = {
  outline: '○',
  drafting: '◐',
  revised: '◑',
  done: '●',
}

export function Outline({
  outline, activeSceneId, onSelectScene, onAddChapter, onAddScene, onMoveScene, onRenameScene, onRenameChapter, onSetRole,
  canWrite = () => true, canShape = true, className,
}: Props) {
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

  const move = (chapterId: string, scenes: Scene[], index: number, delta: number) => {
    const next = [...scenes]
    const target = index + delta
    if (target < 0 || target >= next.length) return
    ;[next[index], next[target]] = [next[target], next[index]]
    onMoveScene(chapterId, next.map((s) => s.id))
  }

  let numbered = 0

  return (
    <nav className={className ? `outline ${className}` : 'outline'}>
      <header className="outline__header">
        <h2>{outline.title}</h2>
        <p className="outline__meta">
          {outline.wordCount.toLocaleString()} words · {outline.status}
        </p>
      </header>

      {outline.chapters.map((chapter) => {
        const writable = canWrite(chapter.id)
        // Front and back matter are not numbered, and do not take a number
        // from the chapters around them.
        const shown =
          chapter.role === 'chapter'
            ? chapterName(chapter.title, ++numbered)
            : chapter.title.trim() || (chapter.role === 'front' ? 'Front matter' : 'Back matter')
        return (
        <section
          key={chapter.id}
          className={writable ? 'outline__chapter' : 'outline__chapter is-read-only'}
        >
          <h3>
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
            {chapter.role !== 'chapter' && (
              <span className="outline__badge" title="Not numbered; included in an export with the front or back matter">
                {chapter.role === 'front' ? 'front matter' : 'back matter'}
              </span>
            )}
            {writable && onSetRole && editing !== chapter.id && (
              <select
                className="outline__role"
                aria-label={`Role of ${shown}`}
                title="Chapter, front matter or back matter"
                value={chapter.role}
                onChange={(e) => onSetRole(chapter.id, e.target.value as 'chapter' | 'front' | 'back')}
              >
                <option value="chapter">Chapter</option>
                <option value="front">Front matter</option>
                <option value="back">Back matter</option>
              </select>
            )}
            {!writable && <span className="outline__lock" title="You can read this chapter but not change it"> · read only</span>}
          </h3>
          <ul>
            {chapter.scenes.map((scene, index) => (
              <li
                key={scene.id}
                className={scene.id === activeSceneId ? 'outline__scene is-active' : 'outline__scene'}
              >
                {editing === scene.id ? (
                  renameField(scene.id, sceneName(scene.title, index + 1), scene.title, (t) => onRenameScene(scene.id, t))
                ) : (
                <button type="button" className="outline__scene-btn" onClick={() => onSelectScene(scene)}>
                  <span className="outline__dot" title={scene.status}>{STATUS_DOT[scene.status]}</span>
                  <span
                    className={isUntitled(scene.title) ? 'outline__title is-placeholder' : 'outline__title'}
                    title={writable ? 'Double-click to name it' : undefined}
                    onDoubleClick={() => writable && edit(scene.id, sceneName(scene.title, index + 1))}
                  >
                    {sceneName(scene.title, index + 1)}
                  </span>
                  <span className="outline__words">{scene.wordCount}</span>
                </button>
                )}
                {writable && (
                  <span className="outline__move">
                    <button type="button" onClick={() => move(chapter.id, chapter.scenes, index, -1)} title="Move up">↑</button>
                    <button type="button" onClick={() => move(chapter.id, chapter.scenes, index, 1)} title="Move down">↓</button>
                  </span>
                )}
              </li>
            ))}
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
        </section>
        )
      })}

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

