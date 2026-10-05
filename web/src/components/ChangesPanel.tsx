import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { Scene, StaleChange, TrackedChange } from '../api/types'

interface Props {
  scene: Scene
  /** The current selection, if any, so a suggestion can be made from it. */
  selection: { from: number; to: number; quote: string } | null
  /** Called after anything that changed the document, so the editor can reload. */
  onApplied: () => void
  /** Whether this person may propose changes here. */
  canPropose?: boolean
  /** Whether this person decides what lands — accepting is writing. */
  canResolve?: boolean
}

/**
 * The replacement text a proposal carries.
 *
 * It is stored as JSON, so a plain string arrives quoted. Parsing it back is
 * the difference between showing `untroubled` and `"untroubled"`.
 */
function payloadText(payload: string | null | undefined): string {
  if (!payload || payload === 'null') return ''
  try {
    const parsed = JSON.parse(payload)
    return typeof parsed === 'string' ? parsed : payload
  } catch {
    return payload
  }
}

const KIND_LABEL: Record<TrackedChange['kind'], string> = {
  replace: 'Replace',
  insert: 'Insert',
  delete: 'Delete',
  format: 'Format',
}

/**
 * Proposed edits, and what happens to them.
 *
 * Accepting applies the change on the server, so the result is the same from
 * any client and a batch is one operation rather than twenty round trips
 * through an editor.
 */
export function ChangesPanel({
  scene, selection, onApplied, canPropose = true, canResolve = true,
}: Props) {
  const [changes, setChanges] = useState<TrackedChange[]>([])
  const [stale, setStale] = useState<StaleChange[]>([])
  const [suggesting, setSuggesting] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.changes.list(scene.id, 'pending').then(setChanges).catch(() => setChanges([]))
  }, [scene.id])

  useEffect(load, [load])

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true)
    setError(null)
    try {
      await work()
      load()
      onApplied()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not do that')
    } finally {
      setBusy(false)
    }
  }

  const resolveAll = (action: 'accept' | 'reject') =>
    run(async () => {
      const result = await api.changes.resolveAll(scene.id, action)
      // Stale ones stay pending; saying which and why is the whole point of
      // refusing rather than guessing.
      setStale(result.stale)
    })

  return (
    <div className="changes">
      {error && <p className="error">{error}</p>}

      {!canPropose && (
        <p className="field__hint">You can read what has been proposed here, not add to it.</p>
      )}
      {canPropose && selection && !suggesting && (
        <button type="button" className="changes__suggest" onClick={() => setSuggesting(true)}>
          Suggest a change to “{selection.quote.slice(0, 30)}{selection.quote.length > 30 ? '…' : ''}”
        </button>
      )}
      {canPropose && !selection && !suggesting && (
        <p className="field__hint">Select a passage in the scene to suggest a change to it.</p>
      )}

      {suggesting && selection && (
        <SuggestForm
          scene={scene}
          selection={selection}
          onDone={() => { setSuggesting(false); load() }}
          onCancel={() => setSuggesting(false)}
        />
      )}

      {canResolve && changes.length > 0 && (
        <div className="changes__bulk">
          <button type="button" disabled={busy} onClick={() => resolveAll('accept')}>
            Accept all {changes.length}
          </button>
          <button type="button" disabled={busy} onClick={() => resolveAll('reject')}>
            Reject all
          </button>
        </div>
      )}

      {stale.length > 0 && (
        <div className="changes__stale">
          <strong>{stale.length} could not be placed and are still waiting:</strong>
          <ul>{stale.map((s) => <li key={s.changeId}>{s.reason}</li>)}</ul>
        </div>
      )}

      {changes.length === 0 && <p className="empty">No changes proposed.</p>}

      <ul className="changes__list">
        {changes.map((change) => (
          <li key={change.id} className="change">
            <div className="change__head">
              <span className={`badge badge--${change.kind}`}>{KIND_LABEL[change.kind]}</span>
              {change.note && <span className="change__note">{change.note}</span>}
            </div>
            {change.quote && (
              <p className="change__quote">
                <del>{change.quote}</del>
                {change.kind !== 'delete' && payloadText(change.payload) && (
                  <ins>{payloadText(change.payload)}</ins>
                )}
              </p>
            )}
            {canResolve && (
            <div className="change__actions">
              <button type="button" disabled={busy} onClick={() => run(() => api.changes.accept(change.id))}>
                Accept
              </button>
              <button type="button" disabled={busy} onClick={() => run(() => api.changes.reject(change.id))}>
                Reject
              </button>
            </div>
            )}
          </li>
        ))}
      </ul>
    </div>
  )
}

function SuggestForm({
  scene, selection, onDone, onCancel,
}: {
  scene: Scene
  selection: { from: number; to: number; quote: string }
  onDone: () => void
  onCancel: () => void
}) {
  const [replacement, setReplacement] = useState(selection.quote)
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  return (
    <form
      className="changes__form"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        try {
          // The hash is read fresh rather than taken from props: it decides
          // whether the proposal's positions can be trusted later.
          const fresh = await api.scenes.get(scene.id)
          await api.changes.create(scene.id, {
            kind: replacement.trim() === '' ? 'delete' : 'replace',
            anchorFrom: selection.from,
            anchorTo: selection.to,
            quote: selection.quote,
            payload: replacement,
            note: note || undefined,
            baseHash: fresh.contentHash,
          })
          onDone()
        } finally {
          setBusy(false)
        }
      }}
    >
      <blockquote className="change__quote">{selection.quote}</blockquote>
      <label>
        Replace with
        <textarea
          rows={2}
          value={replacement}
          placeholder="Leave empty to suggest deleting it"
          onChange={(e) => setReplacement(e.target.value)}
        />
      </label>
      <label>
        Why
        <input value={note} placeholder="optional" onChange={(e) => setNote(e.target.value)} />
      </label>
      <div className="change__actions">
        <button type="submit" disabled={busy}>Propose</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}
