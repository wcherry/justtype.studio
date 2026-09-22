import { useState } from 'react'
import type { WritingSummary } from '../api/types'

interface Props {
  summary: WritingSummary | null
  sessionWords: number
  sceneWords: number
  sceneTarget: number | null
  onSetDailyGoal: (goal: number | null) => void | Promise<void>
  onSetSceneTarget: (target: number | null) => void | Promise<void>
}

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
  summary, sessionWords, sceneWords, sceneTarget, onSetDailyGoal, onSetSceneTarget,
}: Props) {
  const [editing, setEditing] = useState<'daily' | 'scene' | null>(null)

  const today = summary?.wordsToday ?? 0
  const goal = summary?.dailyWordGoal ?? null
  const progress = goal ? Math.max(0, Math.min(1, today / goal)) : 0
  const met = goal !== null && today >= goal
  const streak = summary?.streak ?? 0

  return (
    <div className="goals">
      <div className="goals__row">
        <span className="goals__stat" title="Words added across this work since you opened it">
          <strong>{sessionWords >= 0 ? '+' : ''}{sessionWords.toLocaleString()}</strong> this sitting
        </span>

        <button type="button" className="goals__stat goals__button" onClick={() => setEditing(editing === 'daily' ? null : 'daily')}>
          <strong>{today.toLocaleString()}</strong>
          {goal ? ` of ${goal.toLocaleString()} today` : ' today'}
        </button>

        {streak > 0 && (
          <span className="goals__stat goals__streak" title="Consecutive days you have written">
            {plural(streak, 'day')} running
          </span>
        )}

        <span className="goals__spacer" />

        <button type="button" className="goals__stat goals__button" onClick={() => setEditing(editing === 'scene' ? null : 'scene')}>
          <strong>{sceneWords.toLocaleString()}</strong>
          {sceneTarget ? ` of ${sceneTarget.toLocaleString()} in this scene` : ' in this scene'}
        </button>
      </div>

      {goal !== null && (
        <div className={met ? 'goals__meter is-met' : 'goals__meter'}>
          <div className="goals__fill" style={{ width: `${progress * 100}%` }} />
        </div>
      )}
      {met && <p className="goals__met">Target met — {plural(today - goal, 'word')} past it.</p>}

      {editing === 'daily' && (
        <TargetForm
          label="Words per day"
          current={goal}
          onCancel={() => setEditing(null)}
          onSubmit={async (value) => { await onSetDailyGoal(value); setEditing(null) }}
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
          type="number" min={1} step={50} autoFocus
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
