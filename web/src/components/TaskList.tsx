import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { TaskView } from '../api/types'

interface Props {
  workId: string
  /** The scene open in the editor, so a new task lands where the writer is. */
  sceneId: string | null
  onOpenScene: (sceneId: string) => void
}

/**
 * What this draft still owes.
 *
 * A review ends with forty notes and no plan. This is the step between: the
 * author decides a note is real, and it becomes a line of work against the
 * scene — which outlives the publication it came from, because the next draft
 * is where it gets answered.
 */
export function TaskList({ workId, sceneId, onOpenScene }: Props) {
  const [tasks, setTasks] = useState<TaskView[]>([])
  const [showDone, setShowDone] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [body, setBody] = useState('')

  const load = useCallback(() => {
    void api.tasks
      .list(workId, { state: showDone ? 'all' : 'open' })
      .then(setTasks)
      .catch(() => setTasks([]))
      .finally(() => setLoaded(true))
  }, [workId, showDone])

  useEffect(load, [load])

  const open = tasks.filter((t) => t.state === 'open')

  return (
    <div className="tasks">
      <div className="tasks__summary">
        <span>{open.length} to do</span>
        <button type="button" className="linklike" onClick={() => setShowDone((d) => !d)}>
          {showDone ? 'Hide finished' : 'Show finished'}
        </button>
      </div>

      {loaded && tasks.length === 0 && (
        <p className="empty">Nothing on the list. Notes you accept from a review land here.</p>
      )}

      <ul className="tasks__list">
        {tasks.map((task) => (
          <li key={task.id} className={task.state === 'done' ? 'is-done' : undefined}>
            <label>
              <input
                type="checkbox"
                checked={task.state === 'done'}
                onChange={async () => {
                  await api.tasks.update(task.id, {
                    state: task.state === 'done' ? 'open' : 'done',
                  })
                  load()
                }}
              />
              <span>{task.body}</span>
            </label>
            <div className="tasks__meta">
              {task.source && (
                <span className="field__hint">from {task.source.author.displayName}</span>
              )}
              {task.sceneId && task.sceneId !== sceneId && (
                <button type="button" className="linklike" onClick={() => onOpenScene(task.sceneId!)}>
                  Go there
                </button>
              )}
              <button
                type="button"
                className="linklike"
                onClick={async () => {
                  await api.tasks.remove(task.id)
                  load()
                }}
              >
                Drop
              </button>
            </div>
            {task.source?.quote && <blockquote>{task.source.quote}</blockquote>}
          </li>
        ))}
      </ul>

      <form
        className="tasks__new"
        onSubmit={async (event) => {
          event.preventDefault()
          if (!body.trim()) return
          await api.tasks.create(workId, { body: body.trim(), sceneId: sceneId ?? undefined })
          setBody('')
          load()
        }}
      >
        <input
          value={body}
          placeholder="Chapter two needs to be colder"
          onChange={(e) => setBody(e.target.value)}
        />
        <button type="submit" disabled={!body.trim()}>Add</button>
      </form>
    </div>
  )
}
