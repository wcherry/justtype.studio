import { useState } from 'react'
import { Link } from 'react-router-dom'
import { staff } from '../../api/client'
import type { BulkDeactivateRequest, BulkDeactivateResponse, SignupGroup } from '../../api/types'
import { Pill, ReasonButton, errorText, useLoad, when } from './common'

const WINDOWS: [number, string][] = [
  [24, 'Last 24 hours'],
  [24 * 7, 'Last 7 days'],
  [24 * 30, 'Last 30 days'],
]

/** A `datetime-local` value, in the browser's time, as an instant the server reads. */
function instant(local: string): string | undefined {
  if (!local) return undefined
  const d = new Date(local)
  return Number.isNaN(d.getTime()) ? undefined : d.toISOString()
}

/**
 * Where recent sign-ups came from, and deactivating a run of scripted ones in
 * one go: preview what a filter matches, then deactivate exactly that.
 */
export function AdminSignupsPage() {
  const [hours, setHours] = useState(24)
  const summary = useLoad(() => staff.signups(hours), [hours])

  const [ip, setIp] = useState('')
  const [domain, setDomain] = useState('')
  const [after, setAfter] = useState('')
  const [before, setBefore] = useState('')
  const [includeVerified, setIncludeVerified] = useState(false)
  /** The filter that was previewed, sent again unchanged to act. */
  const [previewed, setPreviewed] = useState<BulkDeactivateRequest | null>(null)
  const [preview, setPreview] = useState<BulkDeactivateResponse | null>(null)
  const [done, setDone] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const filter = (): BulkDeactivateRequest => ({
    ip: ip.trim() || undefined,
    emailDomain: domain.trim() || undefined,
    createdAfter: instant(after),
    createdBefore: instant(before),
    includeVerified,
  })
  // Any change to the form makes the preview stale.
  const changed = <T,>(set: (v: T) => void) => (v: T) => {
    set(v)
    setPreview(null)
    setPreviewed(null)
  }

  const runPreview = async (input: BulkDeactivateRequest) => {
    setBusy(true)
    setError(null)
    setDone(null)
    try {
      setPreview(await staff.bulkDeactivate({ ...input, dryRun: true }))
      setPreviewed(input)
    } catch (e) {
      setError(errorText(e))
      setPreview(null)
      setPreviewed(null)
    } finally {
      setBusy(false)
    }
  }

  const pick = (by: 'ip' | 'domain', key: string) => {
    const since = new Date(Date.now() - hours * 3600_000)
    // `datetime-local` wants local time without a zone.
    const local = new Date(since.getTime() - since.getTimezoneOffset() * 60_000).toISOString().slice(0, 16)
    const input: BulkDeactivateRequest = {
      ip: by === 'ip' ? key : undefined,
      emailDomain: by === 'domain' ? key : undefined,
      createdAfter: since.toISOString(),
      includeVerified: false,
    }
    setIp(by === 'ip' ? key : '')
    setDomain(by === 'domain' ? key : '')
    setAfter(local)
    setBefore('')
    setIncludeVerified(false)
    void runPreview(input)
  }

  const table = (title: string, rows: SignupGroup[] | undefined, by: 'ip' | 'domain') => (
    <section className="prompts__section">
      <h2>{title}</h2>
      {rows && rows.length === 0 ? (
        <p className="empty">None in this window.</p>
      ) : (
        <div className="admin__table-wrap">
          <table className="admin__table">
            <thead>
              <tr><th>{by === 'ip' ? 'Address' : 'Domain'}</th><th>Accounts</th><th>Unverified</th><th /></tr>
            </thead>
            <tbody>
              {rows?.map((g) => (
                <tr key={g.key}>
                  <td><code>{g.key}</code></td>
                  <td>{g.total}</td>
                  <td>{g.unverified}</td>
                  <td>
                    <button type="button" className="linklike" onClick={() => pick(by, g.key)}>Preview cleanup</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )

  const s = summary.data
  return (
    <main className="feed admin">
      <header className="feed__header"><h1>Sign-ups</h1></header>
      <p className="field__hint">
        Where new accounts came from. Addresses are recorded from the sign-up guard onwards; older accounts show
        under their email domain only.
      </p>
      <div className="admin__filters">
        <select aria-label="Window" value={hours} onChange={(e) => setHours(Number(e.target.value))}>
          {WINDOWS.map(([h, label]) => <option key={h} value={h}>{label}</option>)}
        </select>
      </div>
      {summary.error && <p className="error">{summary.error}</p>}
      <div className="admin__cards">
        <div className="admin__card"><strong>{s?.total ?? '…'}</strong><span>new accounts</span></div>
        <div className="admin__card"><strong>{s?.unverified ?? '…'}</strong><span>not yet verified</span></div>
      </div>
      {table('Busiest addresses', s?.byIp, 'ip')}
      {table('Busiest email domains', s?.byDomain, 'domain')}

      <section className="prompts__section">
        <h2>Deactivate in bulk</h2>
        <p className="field__hint">
          Deactivated accounts are signed out and can't sign in; each can be reactivated from its page. Staff and
          your own account are never matched, and accounts with a verified address only when you say so.
        </p>
        <form
          className="admin__reason"
          onSubmit={(e) => { e.preventDefault(); void runPreview(filter()) }}
        >
          <label>Address<input value={ip} placeholder="203.0.113.7" onChange={(e) => changed(setIp)(e.target.value)} /></label>
          <label>Email domain<input value={domain} placeholder="spam.example" onChange={(e) => changed(setDomain)(e.target.value)} /></label>
          <label>Made from<input type="datetime-local" value={after} onChange={(e) => changed(setAfter)(e.target.value)} /></label>
          <label>Made before<input type="datetime-local" value={before} onChange={(e) => changed(setBefore)(e.target.value)} /></label>
          <label className="auth__check">
            <input type="checkbox" checked={includeVerified} onChange={(e) => changed(setIncludeVerified)(e.target.checked)} />
            <span>Include accounts that verified their address</span>
          </label>
          <div className="critique__actions">
            <button type="submit" disabled={busy}>Preview</button>
          </div>
        </form>
        {error && <p className="error">{error}</p>}
        {done !== null && <p>{done} account{done === 1 ? '' : 's'} deactivated.</p>}
        {preview && previewed && (
          <>
            <p>
              <strong>{preview.matched}</strong> account{preview.matched === 1 ? '' : 's'} match
              {preview.matched > preview.sample.length && ` — the newest ${preview.sample.length} are shown`}.
            </p>
            {preview.sample.length > 0 && (
              <div className="admin__table-wrap">
                <table className="admin__table">
                  <thead>
                    <tr><th>Account</th><th>Email</th><th>Made</th><th>Address</th><th /></tr>
                  </thead>
                  <tbody>
                    {preview.sample.map((a) => (
                      <tr key={a.id}>
                        <td><Link to={`/admin/accounts/${a.id}`}>{a.displayName}</Link></td>
                        <td>{a.email}</td>
                        <td>{when(a.createdAt)}</td>
                        <td>{a.signupIp ? <code>{a.signupIp}</code> : '—'}</td>
                        <td>{a.emailVerified && <Pill tone="good">verified</Pill>}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {preview.matched > 0 && (
              <ReasonButton
                danger
                label={`Deactivate ${preview.matched} account${preview.matched === 1 ? '' : 's'}`}
                prompt="Why? This goes in the log against every account."
                onDo={async (reason) => {
                  const result = await staff.bulkDeactivate({ ...previewed, dryRun: false, reason })
                  setDone(result.deactivated)
                  setPreview(null)
                  setPreviewed(null)
                  await summary.reload()
                }}
              />
            )}
          </>
        )}
      </section>
    </main>
  )
}
