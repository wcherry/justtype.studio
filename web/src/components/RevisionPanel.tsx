import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { SceneRevision } from '../api/types'

interface Props {
  sceneId: string
  /** Bumped by the editor after each save, so the list follows along. */
  refreshKey: number
  onRestored: () => void
}

/**
 * The scene's own drafting history. Every checkpoint the editor took is here,
 * and restoring one is itself a save — so the text being replaced is kept too,
 * and a restore can be undone by restoring what it superseded.
 */
export function RevisionPanel({ sceneId, refreshKey, onRestored }: Props) {
  const [revisions, setRevisions] = useState<SceneRevision[]>([])
  const [expanded, setExpanded] = useState<string | null>(null)
  const [preview, setPreview] = useState<{ id: string; changes: { op: string; value: string }[] } | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.scenes.revisions(sceneId).then(setRevisions).catch(() => setRevisions([]))
  }, [sceneId])

  useEffect(load, [load, refreshKey])

  const show = async (revisionId: string) => {
    if (expanded === revisionId) {
      setExpanded(null)
      return
    }
    setExpanded(revisionId)
    setPreview(null)
    try {
      const full = await api.scenes.revision(sceneId, revisionId)
      setPreview({ id: revisionId, changes: full.changes })
    } catch {
      setPreview(null)
    }
  }

  const restore = async (revisionId: string) => {
    setBusy(true)
    setError(null)
    try {
      await api.scenes.restoreRevision(sceneId, revisionId)
      onRestored()
      load()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'could not restore that revision')
    } finally {
      setBusy(false)
    }
  }

  if (revisions.length === 0) {
    return <p className="empty">No checkpoints yet. One is kept every half minute while you write.</p>
  }

  return (
    <div className="revisions">
      {error && <p className="error">{error}</p>}
      <ul className="revisions__list">
        {revisions.map((revision, index) => (
          <li key={revision.id} className="revisions__item">
            <button type="button" className="revisions__row" onClick={() => show(revision.id)}>
              <span>{new Date(revision.createdAt).toLocaleString()}</span>
              <span className="revisions__meta">
                {revision.wordCount.toLocaleString()} words
                {index === 0 && <span className="badge">latest</span>}
              </span>
            </button>
            {expanded === revision.id && (
              <div className="revisions__detail">
                {preview?.id === revision.id ? (
                  <p className="diff__text">
                    {/* Diffed against the scene as it stands now, so this reads
                        as "what restoring would change". */}
                    {preview.changes.map((change, i) => (
                      <span key={i} className={`diff__${change.op}`}>
                        {change.value}
                      </span>
                    ))}
                  </p>
                ) : (
                  <p className="empty">Loading…</p>
                )}
                <button type="button" disabled={busy} onClick={() => restore(revision.id)}>
                  Restore this version
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}
