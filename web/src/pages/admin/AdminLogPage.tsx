import { useState } from 'react'
import { Link } from 'react-router-dom'
import { staff } from '../../api/client'
import type { LogLine } from '../../api/types'
import { useAuth } from '../../state/auth'
import { errorText, when } from './common'
import { useEffect } from 'react'

/** Every action staff took, newest first, with who, what, when and why. Nothing here can be changed. */
export function AdminLogPage() {
  const { user } = useAuth()
  const [lines, setLines] = useState<LogLine[]>([])
  const [cursor, setCursor] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [action, setAction] = useState('')
  const [subject, setSubject] = useState('')
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [applied, setApplied] = useState({ action: '', subject: '', from: '', to: '' })

  const load = (after?: string) =>
    staff
      .log({ ...applied, cursor: after })
      .then((page) => {
        setLines((current) => (after ? [...current, ...page.lines] : page.lines))
        setCursor(page.nextCursor ?? null)
      })
      .catch((e) => setError(errorText(e)))
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { void load() }, [applied])

  return (
    <main className="feed admin">
      <header className="feed__header">
        <h1>Moderation log</h1>
        {user?.isAdmin && (
          <button
            type="button"
            onClick={async () => {
              const csv = await staff.logCsv()
              const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }))
              const a = document.createElement('a')
              a.href = url
              a.download = 'moderation-log.csv'
              a.click()
              URL.revokeObjectURL(url)
            }}
          >
            Download CSV
          </button>
        )}
      </header>
      {!user?.isAdmin && <p className="field__hint">You see your own actions, and what was done to an account you open.</p>}
      <form className="admin__filters" onSubmit={(e) => { e.preventDefault(); setApplied({ action, subject, from, to }) }}>
        <input aria-label="Action" placeholder="Action, e.g. content_removed" value={action} onChange={(e) => setAction(e.target.value)} />
        <input aria-label="Account id" placeholder="Account id" value={subject} onChange={(e) => setSubject(e.target.value)} />
        <input aria-label="From" type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
        <input aria-label="To" type="date" value={to} onChange={(e) => setTo(e.target.value)} />
        <button type="submit">Filter</button>
      </form>
      {error && <p className="error">{error}</p>}
      <ul className="admin__list admin__log">
        {lines.map((l) => (
          <li key={l.id}>
            <span className="field__hint">{when(l.createdAt)}</span>{' '}
            <strong>{l.actor?.displayName ?? 'someone'}</strong>
            {l.origin !== 'staff' && <span className="field__hint"> ({l.origin})</span>}{' '}
            {l.action.replace(/_/g, ' ')}
            {l.subject && <> · <Link to={`/admin/accounts/${l.subject.id}`}>{l.subject.displayName}</Link></>}
            {l.detail && <> — {l.detail}</>}
            {l.reason && <em> ({l.reason})</em>}
          </li>
        ))}
      </ul>
      {lines.length === 0 && <p className="empty">Nothing yet.</p>}
      {cursor && <button type="button" onClick={() => void load(cursor)}>Show more</button>}
    </main>
  )
}
