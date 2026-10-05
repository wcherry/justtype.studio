import { useEffect, useState } from 'react'
import { api } from '../api/client'
import type { ActivityKind, ActivityView } from '../api/types'

const VERB: Record<ActivityKind, string> = {
  scene_edited: 'edited',
  scene_added: 'added the scene',
  scene_removed: 'removed the scene',
  chapter_added: 'added the chapter',
  chapter_removed: 'removed the chapter',
  change_proposed: 'proposed a change to',
  change_accepted: 'accepted a change to',
  change_rejected: 'turned down a change to',
  task_added: 'added to the to-do list:',
  task_done: 'ticked off',
  version_saved: 'snapshotted',
  version_published: 'published',
  publication_opened: 'offered',
  collaborator_added: 'added',
  collaborator_changed: 'changed',
  collaborator_removed: 'took off',
}

function when(iso: string): string {
  const then = new Date(iso)
  const minutes = Math.round((Date.now() - then.getTime()) / 60_000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes} min ago`
  if (minutes < 60 * 24) return `${Math.round(minutes / 60)} h ago`
  return then.toLocaleDateString()
}

/** What has happened to the work, and who did it. Newest first. */
export function ActivityPanel({ workId, refreshKey }: { workId: string; refreshKey: number }) {
  const [rows, setRows] = useState<ActivityView[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api.collaborators
      .activity(workId)
      .then(setRows)
      .catch((e) => setError(e.message))
  }, [workId, refreshKey])

  if (error) return <p className="error">{error}</p>
  if (!rows) return <p className="empty">Loading…</p>
  if (rows.length === 0) return <p className="empty">Nothing has happened here yet.</p>

  return (
    <ul className="activity">
      {rows.map((row) => (
        <li key={row.id} className="activity__row">
          <span>
            <strong>{row.actor.displayName}</strong> {VERB[row.kind]} <em>{row.subject}</em>
            {row.times > 1 && <span className="field__hint"> · {row.times} saves</span>}
          </span>
          <time className="activity__when" dateTime={row.updatedAt}>{when(row.updatedAt)}</time>
        </li>
      ))}
    </ul>
  )
}
