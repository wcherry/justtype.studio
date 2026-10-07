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
  outline, activeSceneId, onSelectScene, onAddChapter, onAddScene, onMoveScene, onRenameScene,
  canWrite = () => true, canShape = true, className,
}: Props) {
  const move = (chapterId: string, scenes: Scene[], index: number, delta: number) => {
    const next = [...scenes]
    const target = index + delta
    if (target < 0 || target >= next.length) return
    ;[next[index], next[target]] = [next[target], next[index]]
    onMoveScene(chapterId, next.map((s) => s.id))
  }

  return (
    <nav className={className ? `outline ${className}` : 'outline'}>
      <header className="outline__header">
        <h2>{outline.title}</h2>
        <p className="outline__meta">
          {outline.wordCount.toLocaleString()} words · {outline.status}
        </p>
      </header>

      {outline.chapters.map((chapter, chapterIndex) => {
        const writable = canWrite(chapter.id)
        return (
        <section
          key={chapter.id}
          className={writable ? 'outline__chapter' : 'outline__chapter is-read-only'}
        >
          <h3>
            <span className={isUntitled(chapter.title) ? 'is-placeholder' : undefined}>
              {chapterName(chapter.title, chapterIndex + 1)}
            </span>
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
                    className={isUntitled(scene.title) ? 'outline__title is-placeholder' : 'outline__title'}
                    title={writable ? 'Double-click to name it' : undefined}
                    onDoubleClick={() => {
                      if (!writable) return
                      // Emptied, it goes back to being called by its place.
                      const title = window.prompt(`Name for ${sceneName(scene.title, index + 1)}`, scene.title)
                      if (title !== null && title.trim() !== scene.title.trim()) onRenameScene(scene.id, title.trim())
                    }}
                  >
                    {sceneName(scene.title, index + 1)}
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

