import { useEffect, useState } from 'react'
import { api } from '../api/client'
import type { SceneHit } from '../api/types'
import { Snippet } from './Snippet'

/** Words anywhere in the work's working text, best matches first. */
export function FindPanel({ workId, onOpen }: { workId: string; onOpen: (sceneId: string) => void }) {
  const [q, setQ] = useState('')
  const [hits, setHits] = useState<SceneHit[] | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Searched as the writer types, a moment after they stop.
  useEffect(() => {
    if (!q.trim()) {
      setHits(null)
      return
    }
    const timer = window.setTimeout(() => {
      api.works.search(workId, q)
        .then((next) => { setHits(next); setError(null) })
        .catch((e) => setError(e.message))
    }, 250)
    return () => window.clearTimeout(timer)
  }, [workId, q])

  return (
    <div className="find">
      <input
        autoFocus
        type="search"
        placeholder="Find in this work…"
        value={q}
        onChange={(e) => setQ(e.target.value)}
      />
      {error && <p className="error">{error}</p>}
      {hits && hits.length === 0 && <p className="empty">Nothing matches.</p>}
      <ul className="find__hits">
        {hits?.map((hit) => (
          <li key={hit.sceneId}>
            <button type="button" className="find__hit" onClick={() => onOpen(hit.sceneId)}>
              <strong>{hit.title}</strong>
              <Snippet text={hit.snippet} />
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
