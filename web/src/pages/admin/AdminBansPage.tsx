import { useState } from 'react'
import { staff } from '../../api/client'
import { ReasonButton, day, errorText, useLoad } from './common'

/** Addresses that can never sign up again, and domains nobody can sign up from. */
export function AdminBansPage() {
  const [q, setQ] = useState('')
  const [search, setSearch] = useState('')
  const [removed, setRemoved] = useState(false)
  const bans = useLoad(() => staff.bans({ q: search, includeRemoved: removed || undefined }), [search, removed])
  const domains = useLoad(() => staff.domains(), [])
  const [email, setEmail] = useState('')
  const [code, setCode] = useState('')
  const [text, setText] = useState('')
  const [domain, setDomain] = useState('')
  const [note, setNote] = useState('')
  const [error, setError] = useState<string | null>(null)

  return (
    <main className="feed admin">
      <header className="feed__header"><h1>Permanent bans</h1></header>
      <p className="field__hint">
        The address and the reason are kept forever, after the account is gone, so that address can never sign up
        again. Gmail dots and +tags count as the same address.
      </p>
      {error && <p className="error">{error}</p>}
      <form className="admin__filters" onSubmit={(e) => { e.preventDefault(); setSearch(q.trim()) }}>
        <input type="search" aria-label="Search" placeholder="Address or name" value={q} onChange={(e) => setQ(e.target.value)} />
        <label className="auth__check"><input type="checkbox" checked={removed} onChange={(e) => setRemoved(e.target.checked)} /><span>Include removed</span></label>
        <button type="submit">Search</button>
      </form>
      <ul className="admin__list">
        {bans.data?.map((b) => (
          <li key={b.id} className="admin__restriction">
            <p>
              <strong>{b.email}</strong> {b.displayName && `(${b.displayName})`} · listed {day(b.createdAt)}
              {b.removedAt && <> · <em>removed {day(b.removedAt)}: {b.removedReason}</em></>}
            </p>
            <p>Guideline {b.reasonCode}: {b.reasonText}</p>
            {!b.removedAt && (
              <ReasonButton
                label="Remove from the list"
                prompt="Why is this address allowed back? This goes in the log."
                onDo={async (reason) => { await staff.removeBan(b.id, reason); await bans.reload() }}
              />
            )}
          </li>
        ))}
      </ul>
      {bans.data?.length === 0 && <p className="empty">Nobody is listed.</p>}

      <section className="prompts__section">
        <h2>Add an address by hand</h2>
        <form
          className="admin__reason"
          onSubmit={async (event) => {
            event.preventDefault()
            setError(null)
            try {
              await staff.addBan({ email, reasonCode: code, reasonText: text })
              setEmail('')
              setCode('')
              setText('')
              await bans.reload()
            } catch (e) {
              setError(errorText(e))
            }
          }}
        >
          <label>Address<input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required /></label>
          <label>Guidelines section<input value={code} onChange={(e) => setCode(e.target.value)} required /></label>
          <label>Reason<textarea rows={2} value={text} onChange={(e) => setText(e.target.value)} required /></label>
          <button type="submit">Add to the list</button>
        </form>
      </section>

      <section className="prompts__section">
        <h2>Blocked domains</h2>
        <p className="field__hint">Throwaway-mail domains. Nobody can sign up, be invited or be moved to an address there.</p>
        <ul className="admin__list">
          {domains.data?.map((d) => (
            <li key={d.domain}>
              <strong>{d.domain}</strong> {d.note && `— ${d.note}`} ·{' '}
              <button type="button" className="linklike" onClick={async () => { await staff.removeDomain(d.domain); await domains.reload() }}>Unblock</button>
            </li>
          ))}
        </ul>
        <form
          className="admin__add"
          onSubmit={async (event) => {
            event.preventDefault()
            setError(null)
            try {
              await staff.addDomain(domain, note)
              setDomain('')
              setNote('')
              await domains.reload()
            } catch (e) {
              setError(errorText(e))
            }
          }}
        >
          <input aria-label="Domain" placeholder="spam.example" value={domain} onChange={(e) => setDomain(e.target.value)} required />
          <input aria-label="Note" placeholder="Note" value={note} onChange={(e) => setNote(e.target.value)} />
          <button type="submit">Block</button>
        </form>
      </section>
    </main>
  )
}
