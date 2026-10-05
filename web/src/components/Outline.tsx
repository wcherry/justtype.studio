import { useState } from 'react'
import type { Outline as OutlineType, Scene } from '../api/types'

interface Props {
  outline: OutlineType
  activeSceneId: string | null
  onSelectScene: (scene: Scene) => void
  onAddChapter: (title: string) => void
  onAddScene: (chapterId: string, title: string) => void
  onMoveScene: (chapterId: string, sceneIds: string[]) => void
  onRenameScene: (sceneId: string, title: string) => void
  /** Whether this chapter's scenes may be added, moved or renamed. */
  canWrite?: (chapterId: string) => boolean
  /** Whether chapters may be added. */
  canShape?: boolean
}

const STATUS_DOT: Record<Scene['status'], string> = {
  outline: '○',
  drafting: '◐',
  revised: '◑',
  done: '●',
}

export function Outline({
  outline, activeSceneId, onSelectScene, onAddChapter, onAddScene, onMoveScene, onRenameScene,
  canWrite = () => true, canShape = true,
}: Props) {
  const [adding, setAdding] = useState<string | null>(null)
  const [draftTitle, setDraftTitle] = useState('')

  const move = (chapterId: string, scenes: Scene[], index: number, delta: number) => {
    const next = [...scenes]
    const target = index + delta
    if (target < 0 || target >= next.length) return
    ;[next[index], next[target]] = [next[target], next[index]]
    onMoveScene(chapterId, next.map((s) => s.id))
  }

  return (
    <nav className="outline">
      <header className="outline__header">
        <h2>{outline.title}</h2>
        <p className="outline__meta">
          {outline.wordCount.toLocaleString()} words · {outline.status}
        </p>
      </header>

      {outline.chapters.map((chapter) => {
        const writable = canWrite(chapter.id)
        return (
        <section
          key={chapter.id}
          className={writable ? 'outline__chapter' : 'outline__chapter is-read-only'}
        >
          <h3>
            {chapter.title}
            {!writable && <span className="outline__lock" title="You can read this chapter but not change it"> · read only</span>}
          </h3>
          <ul>
            {chapter.scenes.map((scene, index) => (
              <li
                key={scene.id}
                className={scene.id === activeSceneId ? 'outline__scene is-active' : 'outline__scene'}
              >
                <button type="button" className="outline__scene-btn" onClick={() => onSelectScene(scene)}>
                  <span className="outline__dot" title={scene.status}>{STATUS_DOT[scene.status]}</span>
                  <span
                    className="outline__title"
                    onDoubleClick={() => {
                      if (!writable) return
                      const title = window.prompt('Scene title', scene.title)
                      if (title && title !== scene.title) onRenameScene(scene.id, title)
                    }}
                  >
                    {scene.title}
                  </span>
                  <span className="outline__words">{scene.wordCount}</span>
                </button>
                {writable && (
                  <span className="outline__move">
                    <button type="button" onClick={() => move(chapter.id, chapter.scenes, index, -1)} title="Move up">↑</button>
                    <button type="button" onClick={() => move(chapter.id, chapter.scenes, index, 1)} title="Move down">↓</button>
                  </span>
                )}
              </li>
            ))}
          </ul>

          {!writable ? null : adding === chapter.id ? (
            <form
              className="outline__add"
              onSubmit={(event) => {
                event.preventDefault()
                if (draftTitle.trim()) onAddScene(chapter.id, draftTitle.trim())
                setDraftTitle('')
                setAdding(null)
              }}
            >
              <input
                autoFocus
                value={draftTitle}
                placeholder="Scene title"
                onChange={(e) => setDraftTitle(e.target.value)}
                onBlur={() => setAdding(null)}
              />
            </form>
          ) : (
            <button type="button" className="outline__add-btn" onClick={() => setAdding(chapter.id)}>
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
        onClick={() => {
          const title = window.prompt('Chapter title', `Chapter ${outline.chapters.length + 1}`)
          if (title) onAddChapter(title)
        }}
      >
        + Chapter
      </button>
      )}
    </nav>
  )
}
