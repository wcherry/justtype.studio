import { useState } from 'react'
import { api } from '../api/client'
import type { ConflictDetails } from '../offline/queue'
import { discardPending } from '../offline/queue'

interface Props {
  conflict: ConflictDetails
  /** Adopt the server's hash as the new baseline and push the text on screen. */
  onKeepMine: (currentHash: string) => void | Promise<void>
  /** Replace the text on screen with what the server holds. */
  onTakeTheirs: (content: unknown, contentHash: string, wordCount: number) => void
}

/**
 * Shown when a save was refused because the scene changed somewhere else.
 *
 * Deliberately blocking and deliberately two-sided: the one thing that must not
 * happen is a silent choice. Both texts still exist — theirs on the server,
 * yours in the offline queue — so neither button can lose work.
 */
export function ConflictBanner({ conflict, onKeepMine, onTakeTheirs }: Props) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const takeTheirs = async () => {
    setBusy(true)
    setError(null)
    try {
      const theirs = await api.scenes.get(conflict.sceneId)
      await discardPending(conflict.sceneId)
      onTakeTheirs(theirs.content, theirs.contentHash, theirs.wordCount)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'could not load the other version')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="conflict" role="alert">
      <div className="conflict__text">
        <strong>This scene changed somewhere else.</strong>{' '}
        Another tab or device saved {conflict.currentWordCount.toLocaleString()} words at{' '}
        {new Date(conflict.updatedAt).toLocaleTimeString()}. Your version is safe on this
        device until you choose.
      </div>
      <div className="conflict__actions">
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              await onKeepMine(conflict.currentHash)
            } finally {
              setBusy(false)
            }
          }}
        >
          Keep mine
        </button>
        <button type="button" disabled={busy} onClick={takeTheirs}>
          Use theirs
        </button>
      </div>
      {error && <p className="error">{error}</p>}
    </div>
  )
}
