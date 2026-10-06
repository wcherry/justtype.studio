import { useCallback, useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { ApiError, api, downloadAccountExport, tokens } from '../api/client'
import type { AuditEvent, SessionView, TwoFactorSetup } from '../api/types'
import { useAuth } from '../state/auth'

/** The account: its address, its password, its second factor, where it is
 *  signed in, and the data held about it. */
export function SettingsPage() {
  return (
    <main className="feed settings">
      <header className="feed__header">
        <h1>Account</h1>
      </header>
      <AddressSection />
      <PasswordSection />
      <TwoFactorSection />
      <SessionsSection />
      <SummariesSection />
      <DataSection />
    </main>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="settings__section">
      <h2>{title}</h2>
      {children}
    </section>
  )
}

function useRun() {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [note, setNote] = useState<string | null>(null)
  const run = async (work: () => Promise<string | void>) => {
    setBusy(true)
    setError(null)
    setNote(null)
    try {
      const said = await work()
      if (said) setNote(said)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not do that')
    } finally {
      setBusy(false)
    }
  }
  const status = (
    <>
      {error && <p className="error">{error}</p>}
      {note && <p className="field__hint">{note}</p>}
    </>
  )
  return { busy, run, status }
}

function AddressSection() {
  const { user, refreshUser } = useAuth()
  const { busy, run, status } = useRun()
  if (!user) return null
  return (
    <Section title="Address">
      <p>
        {user.email}{' '}
        {user.emailVerifiedAt
          ? <span className="badge badge--public">confirmed</span>
          : <span className="badge">not confirmed</span>}
      </p>
      {!user.emailVerifiedAt && (
        <>
          <p className="field__hint">
            Drafts and books shared with this address reach you once you have confirmed it is
            yours.
          </p>
          <div className="settings__row">
            <button
              type="button"
              disabled={busy}
              onClick={() => run(async () => {
                await api.account.resendVerification()
                return 'Sent. Follow the link in it, then come back.'
              })}
            >
              Send the link again
            </button>
            <button type="button" className="linklike" onClick={() => void refreshUser()}>
              I have followed it
            </button>
          </div>
        </>
      )}
      {status}
    </Section>
  )
}

function PasswordSection() {
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const { busy, run, status } = useRun()
  return (
    <Section title="Password">
      <form
        className="settings__form"
        onSubmit={(event) => {
          event.preventDefault()
          void run(async () => {
            await api.account.changePassword(current, next)
            setCurrent('')
            setNext('')
            return 'Changed. Every other device has been signed out.'
          })
        }}
      >
        <label>
          Current
          <input type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} required />
        </label>
        <label>
          New
          <input type="password" autoComplete="new-password" minLength={8} value={next} onChange={(e) => setNext(e.target.value)} required />
        </label>
        <button type="submit" disabled={busy}>Change it</button>
      </form>
      {status}
    </Section>
  )
}

function TwoFactorSection() {
  const { user, refreshUser } = useAuth()
  const [setup, setSetup] = useState<TwoFactorSetup | null>(null)
  const [code, setCode] = useState('')
  const [password, setPassword] = useState('')
  const [recovery, setRecovery] = useState<string[] | null>(null)
  const { busy, run, status } = useRun()
  if (!user) return null
  const on = !!user.totpEnabledAt

  return (
    <Section title="Two-factor sign-in">
      {recovery ? (
        <>
          <p>
            On. Keep these recovery codes somewhere that is not this computer — each gets you in
            once if you lose the authenticator. They are not shown again.
          </p>
          <ul className="settings__codes">{recovery.map((c) => <li key={c}><code>{c}</code></li>)}</ul>
          <button type="button" onClick={() => { setRecovery(null); void refreshUser() }}>
            I have kept them
          </button>
        </>
      ) : on ? (
        <form
          className="settings__form"
          onSubmit={(event) => {
            event.preventDefault()
            void run(async () => {
              await api.account.disableTwoFactor(password, code)
              setPassword('')
              setCode('')
              await refreshUser()
              return 'Off.'
            })
          }}
        >
          <p>On since {new Date(user.totpEnabledAt!).toLocaleDateString()}. Turning it off takes both factors.</p>
          <label>
            Password
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          </label>
          <label>
            Code, or a recovery code
            <input value={code} inputMode="numeric" onChange={(e) => setCode(e.target.value)} required />
          </label>
          <button type="submit" disabled={busy}>Turn it off</button>
        </form>
      ) : setup ? (
        <form
          className="settings__form"
          onSubmit={(event) => {
            event.preventDefault()
            void run(async () => {
              const result = await api.account.enableTwoFactor(code)
              setRecovery(result.codes)
              setSetup(null)
              setCode('')
            })
          }}
        >
          <p>
            Add this key to an authenticator app — on a phone, the link opens one directly — then
            type the six digits it shows.
          </p>
          <p className="settings__secret"><code>{setup.secret.match(/.{1,4}/g)?.join(' ')}</code></p>
          <p><a href={setup.url}>Open in an authenticator app</a></p>
          <label>
            Code
            <input value={code} inputMode="numeric" autoComplete="one-time-code" onChange={(e) => setCode(e.target.value)} required autoFocus />
          </label>
          <button type="submit" disabled={busy}>Turn it on</button>
        </form>
      ) : (
        <>
          <p className="field__hint">
            A code from your phone as well as your password, so a leaked password alone does not
            get anyone into your manuscripts.
          </p>
          <button
            type="button"
            disabled={busy}
            onClick={() => run(async () => { setSetup(await api.account.setupTwoFactor()) })}
          >
            Set it up
          </button>
        </>
      )}
      {status}
    </Section>
  )
}

function describe(agent: string | null | undefined): string {
  if (!agent) return 'An unknown device'
  const browser = /Firefox\//.test(agent) ? 'Firefox'
    : /Edg\//.test(agent) ? 'Edge'
    : /Chrome\//.test(agent) ? 'Chrome'
    : /Safari\//.test(agent) ? 'Safari'
    : /Scriptorium|CFNetwork/.test(agent) ? 'The iOS app'
    : 'A browser'
  const system = /iPhone|iPad/.test(agent) ? 'iOS'
    : /Mac OS X/.test(agent) ? 'macOS'
    : /Windows/.test(agent) ? 'Windows'
    : /Android/.test(agent) ? 'Android'
    : /Linux/.test(agent) ? 'Linux'
    : null
  return system ? `${browser} on ${system}` : browser
}

function SessionsSection() {
  const [sessions, setSessions] = useState<SessionView[]>([])
  const { busy, run, status } = useRun()
  const load = useCallback(() => {
    api.account.sessions().then(setSessions).catch(() => undefined)
  }, [])
  useEffect(load, [load])

  return (
    <Section title="Signed in">
      <ul className="settings__sessions">
        {sessions.map(({ id, userAgent, ip, createdAt, lastSeenAt, current }) => (
          <li key={id}>
            <div>
              <strong>{describe(userAgent)}</strong>
              {current && <span className="badge badge--public">this one</span>}
              <p className="field__hint">
                Signed in {new Date(createdAt).toLocaleString()} · last active{' '}
                {new Date(lastSeenAt).toLocaleString()}{ip && ` · ${ip}`}
              </p>
            </div>
            {!current && (
              <button
                type="button"
                className="linklike"
                disabled={busy}
                onClick={() => run(async () => { await api.account.revokeSession(id); load() })}
              >
                Sign out
              </button>
            )}
          </li>
        ))}
      </ul>
      {sessions.length > 1 && (
        <button
          type="button"
          disabled={busy}
          onClick={() => run(async () => {
            await api.account.signOutOthers()
            load()
            return 'Every other device is signed out — now, not when its session would have run out.'
          })}
        >
          Sign out everywhere else
        </button>
      )}
      {status}
    </Section>
  )
}

function SummariesSection() {
  const { user, updateUser } = useAuth()
  const [available, setAvailable] = useState(false)
  const { busy, run, status } = useRun()
  useEffect(() => {
    api.features().then((f) => setAvailable(f.summaries)).catch(() => undefined)
  }, [])
  if (!available || !user) return null
  const on = !!user.summariesAllowedAt
  return (
    <Section title="Summaries">
      <p className="field__hint">
        Summaries of a scene, a chapter or the book are written by Claude, a model Anthropic
        runs. When they are on, the text you ask to summarise — only that, only when you ask — is
        sent to Anthropic. Summaries already written stay with your work if you turn this off.
      </p>
      <div className="settings__row">
        <span>{on ? `On since ${new Date(user.summariesAllowedAt!).toLocaleDateString()}` : 'Off'}</span>
        <button
          type="button"
          disabled={busy}
          onClick={() => run(async () => {
            updateUser(on ? await api.summaries.withdraw() : await api.summaries.allow())
          })}
        >
          {on ? 'Turn off' : 'Turn on'}
        </button>
      </div>
      {status}
    </Section>
  )
}

const ACTION_LABEL: Record<string, string> = {
  password_reset: 'Password reset by email',
  password_changed: 'Password changed',
  two_factor_enabled: 'Two-factor sign-in turned on',
  two_factor_disabled: 'Two-factor sign-in turned off',
  session_revoked: 'A device signed out',
  sessions_revoked: 'Other devices signed out',
  account_exported: 'Data downloaded',
  account_deleted: 'Account deleted',
  work_deleted: 'Work deleted',
  publication_created: 'Offered for review',
  publication_changed: 'Publication changed',
  publication_withdrawn: 'Publication withdrawn',
  collaborator_added: 'Co-author added',
  collaborator_changed: 'Co-author’s role changed',
  collaborator_removed: 'Co-author removed',
  reader_invited: 'Readers invited',
  reader_uninvited: 'Invitation withdrawn',
  reader_blocked: 'Reader blocked',
  reader_unblocked: 'Reader unblocked',
  terms_accepted: 'Terms agreed',
  summaries_allowed: 'Summaries turned on',
  summaries_withdrawn: 'Summaries turned off',
  share_link_made: 'Share link made',
  share_links_revoked: 'Share links withdrawn',
}

function DataSection() {
  const { user, logout } = useAuth()
  const navigate = useNavigate()
  const [audit, setAudit] = useState<AuditEvent[] | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [shared, setShared] = useState<string[] | null>(null)
  const { busy, run, status } = useRun()

  return (
    <Section title="Your data">
      <p className="field__hint">
        Everything held about you — your works and every file behind them, what you said on other
        people’s, and the record below — as one zip you can open without us.
      </p>
      <div className="settings__row">
        <button type="button" disabled={busy} onClick={() => run(async () => { await downloadAccountExport() })}>
          Download everything
        </button>
        <button
          type="button"
          className="linklike"
          onClick={() => (audit ? setAudit(null) : api.trust.audit().then(setAudit))}
        >
          {audit ? 'Hide the record' : 'Show the record'}
        </button>
      </div>
      {audit && (
        <ul className="settings__audit">
          {audit.length === 0 && <li className="empty">Nothing recorded yet.</li>}
          {audit.map((event) => (
            <li key={event.id}>
              <span>{ACTION_LABEL[event.action] ?? event.action}</span>
              {event.detail && <span className="field__hint"> · {event.detail}</span>}
              <time className="activity__when">{new Date(event.createdAt).toLocaleString()}</time>
            </li>
          ))}
        </ul>
      )}

      <h3 className="settings__danger">Delete the account</h3>
      {!deleting ? (
        <button type="button" onClick={() => setDeleting(true)}>Delete my account…</button>
      ) : (
        <form
          className="settings__form"
          onSubmit={(event) => {
            event.preventDefault()
            void run(async () => {
              try {
                await api.trust.deleteAccount({
                  password,
                  code: user?.totpEnabledAt ? code : undefined,
                  deleteSharedWorks: shared !== null,
                })
              } catch (e) {
                // Works other people are on go too; say which, and ask again.
                if (e instanceof ApiError && e.status === 409) {
                  const details = e.details as { sharedWorks?: string[] } | undefined
                  setShared(details?.sharedWorks ?? [])
                  return
                }
                throw e
              }
              tokens.clear()
              await logout().catch(() => undefined)
              navigate('/sign-in')
            })
          }}
        >
          <p>
            This deletes your account, every work you started and every file behind them. Notes
            you left on other people’s books are blanked, so the replies under them still read.
            Download everything first if you want a copy. It cannot be undone.
          </p>
          {shared && (
            <p className="error">
              Other people are working on {shared.join(', ')}. Deleting your account deletes{' '}
              {shared.length === 1 ? 'it' : 'them'} for them too. Delete again to go ahead.
            </p>
          )}
          <label>
            Password
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required />
          </label>
          {user?.totpEnabledAt && (
            <label>
              Code from your authenticator
              <input value={code} inputMode="numeric" onChange={(e) => setCode(e.target.value)} required />
            </label>
          )}
          <div className="settings__row">
            <button type="submit" className="danger" disabled={busy}>
              {shared ? 'Delete it, and those works' : 'Delete my account'}
            </button>
            <button type="button" className="linklike" onClick={() => { setDeleting(false); setShared(null) }}>
              Keep it
            </button>
          </div>
        </form>
      )}
      {status}
    </Section>
  )
}
