import { useState } from 'react'

interface Props {
  daily: number | null
  weekly: number | null
  /** The open scene's target; null when no scene is open. */
  scene: { target: number | null } | null
  book: { target: number | null; deadline: string | null } | null
  onSave: (goals: {
    daily: number | null
    weekly: number | null
    scene?: number | null
    book?: { target: number | null; deadline: string | null }
  }) => Promise<void>
}

const parse = (text: string) => {
  const n = Number(text)
  return text.trim() === '' || !(n > 0) ? null : Math.floor(n)
}

/** Every writing goal in one place: set them, change them, or clear them. */
export function GoalsPanel({ daily, weekly, scene, book, onSave }: Props) {
  const [day, setDay] = useState(daily ? String(daily) : '')
  const [week, setWeek] = useState(weekly ? String(weekly) : '')
  const [scn, setScn] = useState(scene?.target ? String(scene.target) : '')
  const [words, setWords] = useState(book?.target ? String(book.target) : '')
  const [date, setDate] = useState(book?.deadline ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const run = async (goals: Parameters<Props['onSave']>[0]) => {
    setBusy(true)
    setError(null)
    try {
      await onSave(goals)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save those goals')
    } finally {
      setBusy(false)
    }
  }

  const reset = async () => {
    setDay('')
    setWeek('')
    setScn('')
    setWords('')
    setDate('')
    await run({
      daily: null,
      weekly: null,
      ...(scene ? { scene: null } : {}),
      ...(book ? { book: { target: null, deadline: null } } : {}),
    })
  }

  return (
    <form
      className="goals-panel"
      onSubmit={(event) => {
        event.preventDefault()
        void run({
          daily: parse(day),
          weekly: parse(week),
          ...(scene ? { scene: parse(scn) } : {}),
          ...(book ? { book: { target: parse(words), deadline: date || null } } : {}),
        })
      }}
    >
      <p className="field__hint">Leave a goal empty for none. Each shows as a filled bar behind its figure.</p>
      <label>
        Words per day
        <input type="number" min={1} step={1} value={day} placeholder="none" onChange={(e) => setDay(e.target.value)} />
      </label>
      <label>
        Words per week (Monday to Sunday)
        <input type="number" min={1} step={1} value={week} placeholder="none" onChange={(e) => setWeek(e.target.value)} />
      </label>
      {scene && (
        <label>
          Words in this scene
          <input type="number" min={1} step={1} value={scn} placeholder="none" onChange={(e) => setScn(e.target.value)} />
        </label>
      )}
      {book && (
        <>
          <label>
            Words in the book
            <input type="number" min={1} step={1} value={words} placeholder="none" onChange={(e) => setWords(e.target.value)} />
          </label>
          <label>
            By
            <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
            <span className="field__hint">Optional. With a date, the bar says what each day needs.</span>
          </label>
        </>
      )}
      {error && <p className="error">{error}</p>}
      <div className="goals__form-actions">
        <button type="submit" disabled={busy}>Save goals</button>
        <button type="button" disabled={busy} onClick={() => void reset()}>Reset all</button>
      </div>
    </form>
  )
}
