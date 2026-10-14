import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import type { StatusOption } from '../api/types'
import { statusGlyph } from '../statuses'

/** The colours a new status is offered, the same ones the server deals out. */
const SWATCHES = ['#c2453d', '#b5649a', '#7a5cc2', '#2f9aa8', '#6b8e23', '#a0522d', '#c9a227', '#5b6b7a']

/**
 * The book's own scene statuses: named, coloured, put in order, removed. They
 * come before the built-in ones in every list, which are shown here too but
 * cannot be changed. Every change goes to the server at once and `onChange`
 * hands back the book's whole list as it now stands.
 */
export function StatusesDialog({
  workId, statuses, inUse, onChange, onClose,
}: {
  workId: string
  statuses: StatusOption[]
  /** How many scenes have each status, by key: what removing one would move. */
  inUse: Record<string, number>
  onChange: (statuses: StatusOption[]) => void
  onClose: () => void
}) {
  const ref = useRef<HTMLDialogElement>(null)
  const [label, setLabel] = useState('')
  const [color, setColor] = useState(SWATCHES[statuses.filter((s) => s.custom).length % SWATCHES.length])
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  /** Names being typed, by key, until they are committed on leaving the field. */
  const [names, setNames] = useState<Record<string, string>>({})

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  const own = statuses.filter((s) => s.custom)
  const builtIn = statuses.filter((s) => !s.custom)

  const run = async (work: () => Promise<StatusOption[]>) => {
    setBusy(true)
    setError(null)
    try {
      onChange(await work())
      return true
    } catch (e) {
      setError(e instanceof Error ? e.message : 'That did not work')
      return false
    } finally {
      setBusy(false)
    }
  }

  const add = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!label.trim()) return
    const ok = await run(() => api.statuses.create(workId, { label: label.trim(), color }))
    if (ok) {
      setLabel('')
      setColor(SWATCHES[(own.length + 1) % SWATCHES.length])
    }
  }

  const move = (index: number, delta: -1 | 1) => {
    const ids = own.map((s) => s.key)
    const [id] = ids.splice(index, 1)
    ids.splice(index + delta, 0, id)
    void run(() => api.statuses.reorder(workId, ids))
  }

  return (
    <dialog
      ref={ref}
      className="dialog statuses"
      aria-labelledby="statuses-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="statuses-title">Scene statuses</h2>
      <p className="field__hint">
        Your own statuses for this book come first in every list, before the four every book has. The colour is
        the dot before the scene’s name in the outline.
      </p>

      <ul className="statuses__list">
        {own.map((status, index) => {
          const count = inUse[status.key] ?? 0
          return (
            <li key={status.key} className="statuses__row">
              <input
                type="color"
                className="statuses__color"
                aria-label={`Colour of ${status.label}`}
                value={status.color}
                disabled={busy}
                // The picker fires as it is dragged; the server hears the colour it is left on.
                onChange={(e) => onChange(statuses.map((s) => (s.key === status.key ? { ...s, color: e.target.value } : s)))}
                onBlur={(e) => void run(() => api.statuses.update(status.key, { color: e.target.value }))}
              />
              <input
                className="statuses__name"
                aria-label="Name"
                value={names[status.key] ?? status.label}
                maxLength={40}
                onChange={(e) => setNames({ ...names, [status.key]: e.target.value })}
                onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                onBlur={async () => {
                  const typed = names[status.key]
                  if (typed === undefined) return
                  const next = { ...names }
                  delete next[status.key]
                  if (typed.trim() && typed.trim() !== status.label) {
                    if (!(await run(() => api.statuses.update(status.key, { label: typed.trim() })))) return
                  }
                  setNames(next)
                }}
              />
              <span className="statuses__count">{count ? `${count} ${count === 1 ? 'scene' : 'scenes'}` : ''}</span>
              <button type="button" className="statuses__icon" aria-label={`Move ${status.label} up`} disabled={busy || index === 0} onClick={() => move(index, -1)}>↑</button>
              <button type="button" className="statuses__icon" aria-label={`Move ${status.label} down`} disabled={busy || index === own.length - 1} onClick={() => move(index, 1)}>↓</button>
              <button
                type="button"
                className="statuses__icon"
                aria-label={`Remove ${status.label}`}
                title={count ? `Its ${count === 1 ? 'scene goes' : `${count} scenes go`} back to Drafting` : 'Remove'}
                disabled={busy}
                onClick={() => void run(() => api.statuses.remove(status.key))}
              >
                ✕
              </button>
            </li>
          )
        })}
        {builtIn.map((status) => (
          <li key={status.key} className="statuses__row is-built-in">
            <span className="statuses__dot" style={{ color: status.color }} aria-hidden="true">{statusGlyph(status)}</span>
            <span className="statuses__name">{status.label}</span>
            <span className="statuses__count">built in</span>
          </li>
        ))}
      </ul>

      <form className="statuses__add" onSubmit={add}>
        <input type="color" className="statuses__color" aria-label="Colour of the new status" value={color} onChange={(e) => setColor(e.target.value)} />
        <input
          className="statuses__name"
          placeholder="A new status, like “Needs research”"
          aria-label="Name of the new status"
          value={label}
          maxLength={40}
          onChange={(e) => setLabel(e.target.value)}
        />
        <button type="submit" disabled={busy || !label.trim()}>Add</button>
      </form>

      {error && <p className="error">{error}</p>}
      <div className="dialog__actions">
        <button type="button" onClick={() => ref.current?.close()}>Done</button>
      </div>
    </dialog>
  )
}
