import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { BlockView } from '../api/types'

/**
 * The people who do not get to read this writer's work.
 *
 * Not a platform ban — one writer's door. A blocked reader cannot open any of
 * their publications, which is a blunter rule than hiding the comments and an
 * easier one to explain.
 */
export function BlockList() {
  const [blocks, setBlocks] = useState<BlockView[]>([])
  const [loaded, setLoaded] = useState(false)

  const load = useCallback(() => {
    void api.moderation
      .blocks()
      .then(setBlocks)
      .catch(() => setBlocks([]))
      .finally(() => setLoaded(true))
  }, [])

  useEffect(load, [load])

  if (loaded && blocks.length === 0) {
    return <p className="empty">Nobody is blocked. You can block a reader from a flagged note.</p>
  }

  return (
    <ul className="review-panel__list">
      {blocks.map((block) => (
        <li key={block.id}>
          <span>{block.reader.displayName}</span>
          <button
            type="button"
            title="Let them back in"
            onClick={async () => {
              setBlocks(await api.moderation.unblock(block.userId))
            }}
          >
            ×
          </button>
        </li>
      ))}
    </ul>
  )
}
