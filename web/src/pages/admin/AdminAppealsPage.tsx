import { useState } from 'react'
import { Link } from 'react-router-dom'
import { staff } from '../../api/client'
import type { AppealRow } from '../../api/types'
import { Pill, day, errorText, useLoad, when } from './common'

/** Appeals waiting for a decision, the ones with a deletion date first. */
export function AdminAppealsPage() {
  const [all, setAll] = useState(false)
  const { data, error, reload } = useLoad(() => staff.appeals(all ? 'all' : 'open'), [all])
  return (
    <main className="feed admin">
      <header className="feed__header">
        <h1>Appeals</h1>
        <label className="auth__check"><input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} /><span>Include decided</span></label>
      </header>
      {error && <p className="error">{error}</p>}
      {data?.length === 0 && <p className="empty">No appeals are waiting.</p>}
      <ul className="admin__list">
        {data?.map((a) => <Appeal key={a.appeal.id} row={a} onDecided={reload} />)}
      </ul>
    </main>
  )
}

function Appeal({ row, onDecided }: { row: AppealRow; onDecided: () => void }) {
  const [message, setMessage] = useState('')
  const [restore, setRestore] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const open = row.appeal.state === 'open'
  const r = row.restriction
  const decide = async (decision: 'upheld' | 'overturned') => {
    setError(null)
    try {
      await staff.decideAppeal(row.appeal.id, { decision, message, restoreContent: restore })
      onDecided()
    } catch (e) {
      setError(errorText(e))
    }
  }
  return (
    <li className={row.daysLeft !== null && row.daysLeft !== undefined ? 'admin__restriction is-in-force' : 'admin__restriction'}>
      <p>
        <Link to={`/admin/accounts/${row.userId}`}><strong>{row.displayName}</strong></Link> ·{' '}
        {r.kind.replace('_', '-')} ({r.scope === 'discussion' ? 'discussions' : 'whole site'}) ·{' '}
        {r.endsAt ? `until ${day(r.endsAt)}` : 'permanent'} · appealed {when(row.appeal.createdAt)}
        {row.daysLeft !== null && row.daysLeft !== undefined && <Pill tone="bad">deleted in {row.daysLeft} day{row.daysLeft === 1 ? '' : 's'} unless decided</Pill>}
        {!open && <Pill>{row.appeal.state}</Pill>}
      </p>
      <p><em>Guideline {r.reasonCode}:</em> {r.reasonText}</p>
      <blockquote className="admin__quote">{row.appeal.body}</blockquote>
      {open ? (
        <>
          <label>
            Message to the writer (required)
            <textarea rows={3} value={message} onChange={(e) => setMessage(e.target.value)} />
          </label>
          <label className="auth__check">
            <input type="checkbox" checked={restore} onChange={(e) => setRestore(e.target.checked)} />
            <span>If overturned, bring back what was removed alongside it</span>
          </label>
          {error && <p className="error">{error}</p>}
          <div className="admin__actions">
            <button type="button" disabled={!message.trim()} onClick={() => void decide('upheld')}>Uphold the restriction</button>
            <button type="button" disabled={!message.trim()} onClick={() => void decide('overturned')}>Overturn it</button>
          </div>
        </>
      ) : (
        <p>Decision: {row.appeal.decisionText}</p>
      )}
    </li>
  )
}
