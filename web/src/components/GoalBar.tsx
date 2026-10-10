import { useState } from 'react'
import type { WritingSummary } from '../api/types'

interface Props {
  summary: WritingSummary | null
  sessionWords: number
  sceneWords: number
  sceneTarget: number | null
  onSetDailyGoal: (goal: number | null) => void | Promise<void>
  onSetWeeklyGoal: (goal: number | null) => void | Promise<void>
  onSetSceneTarget: (target: number | null) => void | Promise<void>
  /** The whole book, against the writer's target for it. */
  bookWords: number
  bookTarget: number | null
  bookDeadline: string | null
  canSetBookTarget: boolean
  onSetBookTarget: (target: number | null, deadline: string | null) => void | Promise<void>
  /**
   * For writing that is not a book (a Daily Prompts answer): its words, so
   * named, in place of the book's and the scene's targets.
   */
  pieceLabel?: string
}

/** Days from `from` to `to`, counting both: the deadline day is a writing day. */
function daysBetween(from: string, to: string): number {
  const a = Date.parse(`${from}T00:00:00Z`)
  const b = Date.parse(`${to}T00:00:00Z`)
  return Math.round((b - a) / 86_400_000) + 1
}

/**
 * What a deadline asks of each remaining day — the only number about a
 * deadline a writer can act on today.
 */
export function pace(words: number, target: number, today: string, deadline: string) {
  const left = Math.max(0, target - words)
  const days = daysBetween(today, deadline)
  if (left === 0) return { left, days, perDay: 0 }
  if (days <= 0) return { left, days, perDay: null }
  return { left, days, perDay: Math.ceil(left / days) }
}

/**
 * A goal's own background is its progress bar: filled from the left in
 * proportion to how far along it is, and green once it is met.
 */
function fillOf(value: number, of: number | null): React.CSSProperties | undefined {
  if (!of) return undefined
  const fraction = Math.max(0, Math.min(1, value / of))
  return { '--fill': `${fraction * 100}%` } as React.CSSProperties
}

const goalClass = (value: number, of: number | null) =>
  `goals__stat goals__button${of ? (value >= of ? ' has-goal is-met' : ' has-goal') : ''}`

function plural(n: number, word: string) {
  return `${n.toLocaleString()} ${word}${Math.abs(n) === 1 ? '' : 's'}`
}

/**
 * Today, this sitting, and how long the writer has kept it up.
 *
 * Progress is reported, never judged: a day spent cutting shows as the negative
 * number it is, and a missed target says nothing at all. The one thing worth
 * celebrating out loud is a target actually met.
 */
export function GoalBar({
  summary, sessionWords, sceneWords, sceneTarget, onSetDailyGoal, onSetWeeklyGoal,
  onSetSceneTarget, bookWords, bookTarget, bookDeadline, canSetBookTarget, onSetBookTarget, pieceLabel,
}: Props) {
  const [editing, setEditing] = useState<'daily' | 'weekly' | 'book' | 'scene' | null>(null)

  const today = summary?.wordsToday ?? 0
  const goal = summary?.dailyWordGoal ?? null
  const met = goal !== null && today >= goal
  const streak = summary?.streak ?? 0
  const week = summary?.wordsThisWeek ?? 0
  const weeklyGoal = summary?.weeklyWordGoal ?? null
  const bookPace =
    bookTarget && bookDeadline && summary
      ? pace(bookWords, bookTarget, summary.today, bookDeadline)
      : null

  return (
    <div className="goals">
      <div className="goals__row">
        <span className="goals__stat" title="Words added across this work since you opened it">
          <strong>{sessionWords >= 0 ? '+' : ''}{sessionWords.toLocaleString()}</strong> this sitting
        </span>

        <button type="button" className={goalClass(today, goal)} style={fillOf(today, goal)} onClick={() => setEditing(editing === 'daily' ? null : 'daily')}>
          <strong>{today.toLocaleString()}</strong>
          {goal ? ` of ${goal.toLocaleString()} today` : ' today'}
        </button>

        <button type="button" className={goalClass(week, weeklyGoal)} style={fillOf(week, weeklyGoal)} onClick={() => setEditing(editing === 'weekly' ? null : 'weekly')}>
          <strong>{week.toLocaleString()}</strong>
          {weeklyGoal ? ` of ${weeklyGoal.toLocaleString()} this week` : ' this week'}
        </button>

        {streak > 0 && (
          <span className="goals__stat goals__streak" title="Consecutive days you have written">
            {plural(streak, 'day')} running
          </span>
        )}

        <span className="goals__spacer" />

        {pieceLabel ? (
          <span className="goals__stat">
            <strong>{sceneWords.toLocaleString()}</strong> {pieceLabel}
          </span>
        ) : (
        <>
        <button
          type="button"
          className={goalClass(bookWords, bookTarget)}
          style={fillOf(bookWords, bookTarget)}
          disabled={!canSetBookTarget && !bookTarget}
          title={canSetBookTarget ? 'Set a target for the book' : undefined}
          onClick={() => canSetBookTarget && setEditing(editing === 'book' ? null : 'book')}
        >
          <strong>{bookWords.toLocaleString()}</strong>
          {bookTarget ? ` of ${bookTarget.toLocaleString()} in the book` : ' in the book'}
          {bookPace && (
            bookPace.left === 0
              ? ' · there'
              : bookPace.perDay === null
                ? ' · past the date'
                : ` · ${bookPace.perDay.toLocaleString()} a day to ${new Date(`${bookDeadline}T00:00:00`).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}`
          )}
        </button>

        <button type="button" className={goalClass(sceneWords, sceneTarget)} style={fillOf(sceneWords, sceneTarget)} onClick={() => setEditing(editing === 'scene' ? null : 'scene')}>
          <strong>{sceneWords.toLocaleString()}</strong>
          {sceneTarget ? ` of ${sceneTarget.toLocaleString()} in this scene` : ' in this scene'}
        </button>
        </>
        )}
      </div>

      {met && <p className="goals__met">Target met — {plural(today - goal, 'word')} past it.</p>}

      {editing === 'daily' && (
        <TargetForm
          label="Words per day"
          current={goal}
          onCancel={() => setEditing(null)}
          onSubmit={async (value) => { await onSetDailyGoal(value); setEditing(null) }}
        />
      )}
      {editing === 'weekly' && (
        <TargetForm
          label="Words per week (Monday to Sunday)"
          current={weeklyGoal}
          onCancel={() => setEditing(null)}
          onSubmit={async (value) => { await onSetWeeklyGoal(value); setEditing(null) }}
        />
      )}
      {editing === 'book' && (
        <BookForm
          target={bookTarget}
          deadline={bookDeadline}
          onCancel={() => setEditing(null)}
          onSubmit={async (target, deadline) => { await onSetBookTarget(target, deadline); setEditing(null) }}
        />
      )}
      {editing === 'scene' && (
        <TargetForm
          label="Words in this scene"
          current={sceneTarget}
          onCancel={() => setEditing(null)}
          onSubmit={async (value) => { await onSetSceneTarget(value); setEditing(null) }}
        />
      )}
    </div>
  )
}

function TargetForm({
  label, current, onSubmit, onCancel,
}: {
  label: string
  current: number | null
  onSubmit: (value: number | null) => void | Promise<void>
  onCancel: () => void
}) {
  const [value, setValue] = useState(current ? String(current) : '')
  const [busy, setBusy] = useState(false)

  return (
    <form
      className="goals__form"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        const parsed = Number(value)
        await onSubmit(value.trim() === '' || parsed <= 0 ? null : parsed)
        setBusy(false)
      }}
    >
      <label>
        {label}
        <input
          type="number" min={1} step={1} autoFocus
          value={value}
          placeholder="none"
          onChange={(e) => setValue(e.target.value)}
        />
      </label>
      <div className="goals__form-actions">
        <button type="submit" disabled={busy}>Save</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}

function BookForm({
  target, deadline, onSubmit, onCancel,
}: {
  target: number | null
  deadline: string | null
  onSubmit: (target: number | null, deadline: string | null) => void | Promise<void>
  onCancel: () => void
}) {
  const [words, setWords] = useState(target ? String(target) : '')
  const [date, setDate] = useState(deadline ?? '')
  const [busy, setBusy] = useState(false)
  return (
    <form
      className="goals__form"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        const parsed = Number(words)
        await onSubmit(words.trim() === '' || parsed <= 0 ? null : parsed, date || null)
        setBusy(false)
      }}
    >
      <label>
        Words in the book
        <input type="number" min={1} step={1} autoFocus value={words} placeholder="none" onChange={(e) => setWords(e.target.value)} />
      </label>
      <label>
        By
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
        <span className="field__hint">Optional. With a date, the bar says what each day needs.</span>
      </label>
      <div className="goals__form-actions">
        <button type="submit" disabled={busy}>Save</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}
