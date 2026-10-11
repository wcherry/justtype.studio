import { useEffect, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { staff } from '../../api/client'
import type { UserRow } from '../../api/types'
import { useAuth } from '../../state/auth'
import { StatusPill, day, errorText } from './common'

/** Find an account by name — or, for administrators, address or id — and open it. */
export function AdminUsersPage() {
  const { user } = useAuth()
  const isAdmin = !!user?.isAdmin
  const [params, setParams] = useSearchParams()
  const [q, setQ] = useState(params.get('q') ?? '')
  const [rows, setRows] = useState<UserRow[]>([])
  const [cursor, setCursor] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [creating, setCreating] = useState(false)
  const status = params.get('status') ?? ''
  const role = params.get('role') ?? ''
  const verified = params.get('verified') ?? ''
  const sort = params.get('sort') ?? 'joined'
  const search = params.get('q') ?? ''

  const load = (after?: string) => {
    setBusy(true)
    setError(null)
    staff
      .users({ q: search, status, role, verified, sort, cursor: after })
      .then((page) => {
        setRows((current) => (after ? [...current, ...page.users] : page.users))
        setCursor(page.nextCursor ?? null)
      })
      .catch((e) => setError(errorText(e)))
      .finally(() => setBusy(false))
  }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => load(), [search, status, role, verified, sort])

  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value)
    else next.delete(key)
    setParams(next, { replace: true })
  }

  return (
    <main className="feed admin">
      <header className="feed__header">
        <h1>Accounts</h1>
        {isAdmin && <button type="button" onClick={() => setCreating((c) => !c)}>New account</button>}
      </header>
      {creating && <CreateAccount onDone={() => { setCreating(false); load() }} />}
      <form
        className="admin__filters"
        onSubmit={(e) => {
          e.preventDefault()
          set('q', q.trim())
        }}
      >
        <input
          type="search"
          aria-label="Search"
          placeholder={isAdmin ? 'Name, email or id' : 'Name'}
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <select aria-label="Status" value={status} onChange={(e) => set('status', e.target.value)}>
          <option value="">Any status</option>
          {['active', 'unverified', 'restricted', 'banned', 'deactivated'].map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
        <select aria-label="Role" value={role} onChange={(e) => set('role', e.target.value)}>
          <option value="">Any role</option>
          <option value="admin">admin</option>
          <option value="moderator">moderator</option>
          <option value="none">writer</option>
        </select>
        <select aria-label="Address confirmed" value={verified} onChange={(e) => set('verified', e.target.value)}>
          <option value="">Confirmed or not</option>
          <option value="true">Confirmed</option>
          <option value="false">Not confirmed</option>
        </select>
        <select aria-label="Sort" value={sort} onChange={(e) => set('sort', e.target.value)}>
          <option value="joined">Newest first</option>
          <option value="last_seen">Last seen</option>
          <option value="reports">Most open reports</option>
        </select>
        <button type="submit">Search</button>
      </form>
      {error && <p className="error">{error}</p>}
      <div className="admin__table-wrap">
        <table className="admin__table">
          <thead>
            <tr>
              <th>Name</th>
              {isAdmin && <th>Email</th>}
              <th>Joined</th>
              <th>Last seen</th>
              <th title="Address confirmed">✓</th>
              <th>2FA</th>
              <th>Role</th>
              <th>Status</th>
              <th>Open reports</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((u) => (
              <tr key={u.id}>
                <td><Link to={`/admin/accounts/${u.id}`}>{u.displayName}</Link></td>
                {isAdmin && <td>{u.email}</td>}
                <td>{day(u.createdAt)}</td>
                <td>{day(u.lastSeenAt)}</td>
                <td>{u.emailVerified ? '✓' : ''}</td>
                <td>{u.twoFactor ? 'on' : 'off'}</td>
                <td>{u.role ?? ''}</td>
                <td><StatusPill status={u.status} ends={u.statusEnds} /></td>
                <td>{u.openReports || ''}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {rows.length === 0 && !busy && <p className="empty">No account matches.</p>}
      {cursor && <button type="button" disabled={busy} onClick={() => load(cursor)}>Show more</button>}
    </main>
  )
}

function CreateAccount({ onDone }: { onDone: () => void }) {
  const [email, setEmail] = useState('')
  const [name, setName] = useState('')
  const [moderator, setModerator] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sent, setSent] = useState<string | null>(null)
  return (
    <form
      className="admin__reason"
      onSubmit={async (event) => {
        event.preventDefault()
        setError(null)
        try {
          await staff.createUser({ email, displayName: name, moderator })
          setSent(email)
          setEmail('')
          setName('')
        } catch (e) {
          setError(errorText(e))
        }
      }}
    >
      <p className="field__hint">
        They are emailed a link to set their own password and agree to the terms. You will never see a password or the link.
      </p>
      <label>Email<input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required /></label>
      <label>Display name<input value={name} onChange={(e) => setName(e.target.value)} required /></label>
      <label className="auth__check">
        <input type="checkbox" checked={moderator} onChange={(e) => setModerator(e.target.checked)} />
        <span>Make them a moderator</span>
      </label>
      {sent && <p className="field__hint">Sent to {sent}.</p>}
      {error && <p className="error">{error}</p>}
      <div className="critique__actions">
        <button type="submit">Create and email</button>
        <button type="button" onClick={onDone}>Close</button>
      </div>
    </form>
  )
}
