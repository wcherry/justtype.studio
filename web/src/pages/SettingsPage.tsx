import { useCallback, useEffect, useState } from 'react'
import { Link, Navigate, useNavigate, useParams } from 'react-router-dom'
import { ApiError, api, downloadAccountExport, tokens } from '../api/client'
import type { AuditEvent, SessionView, Theme, TwoFactorSetup } from '../api/types'
import { Tabs } from '../components/Tabs'
import { useWritingPreferences } from '../editor/preferences'
import { useAuth } from '../state/auth'
import { THEMES, applyTheme, useTheme } from '../state/theme'

type SettingsTab = 'account' | 'general'
const TABS: { key: SettingsTab; label: string }[] = [
  { key: 'account', label: 'Account' },
  { key: 'general', label: 'Settings' },
]

/**
 * Two tabs, each with its own address so a link can go straight to one:
 * the account (its address, password, second factor, where it is signed in,
 * the data held about it) and the settings (how the app looks, what the
 * models may see, how writing behaves, Daily Prompts).
 */
export function SettingsPage() {
  const { tab } = useParams()
  const navigate = useNavigate()
  if (tab !== 'account' && tab !== 'general') return <Navigate to="/settings/account" replace />
  return (
    <main className="feed settings">
      <header className="feed__header">
        <h1>{tab === 'account' ? 'Account' : 'Settings'}</h1>
      </header>
      <Tabs label="Account and settings" tabs={TABS} active={tab} onChange={(key) => navigate(`/settings/${key}`)}>
        {tab === 'account' ? (
          <>
            <AddressSection />
            <PasswordSection />
            <TwoFactorSection />
            <SessionsSection />
            <DataSection />
          </>
        ) : (
          <>
            <AppearanceSection />
            <SummariesSection />
            <AssistantSection />
            <WritingSection />
            <Section title="Daily Prompts">
              <p className="field__hint">
                Whether you take part, and the daily email, are set on <Link to="/prompts#preferences">the Daily Prompts page</Link>.
              </p>
            </Section>
          </>
        )}
      </Tabs>
    </main>
  )
}

/**
 * The theme. Picking one shows it at once and saves it to the account, so
 * every device the writer signs in on looks the same.
 */
function AppearanceSection() {
  const { user, updateUser } = useAuth()
  const { busy, run, status } = useRun()
  const theme = useTheme()
  if (!user) return null
  const choose = (next: Theme) => {
    if (next === theme) return
    const before = theme
    applyTheme(next)
    void run(async () => {
      try {
        updateUser(await api.account.updateSettings({ theme: next }))
      } catch (e) {
        // Not saved: go back to what the account still says, rather than
        // look right here and wrong everywhere else.
        applyTheme(before)
        throw e
      }
    })
  }
  return (
    <Section title="Appearance">
      <div className="theme-grid" role="radiogroup" aria-label="Theme">
        {THEMES.map((t) => (
          <button
            key={t.value}
            type="button"
            role="radio"
            aria-checked={theme === t.value}
            className={theme === t.value ? 'theme-option is-selected' : 'theme-option'}
            disabled={busy}
            onClick={() => choose(t.value)}
          >
            <span className="theme-option__preview" aria-hidden="true">
              {t.value === 'system' ? (
                <>
                  <ThemePage theme="light" />
                  <ThemePage theme="dark" />
                </>
              ) : (
                <ThemePage theme={t.value} />
              )}
            </span>
            <span className="theme-option__label">{t.label}</span>
            <span className="theme-option__hint">{t.hint}</span>
          </button>
        ))}
      </div>
      <p className="field__hint">Kept with your account, so it is the same on every device you sign in on.</p>
      {status}
    </Section>
  )
}

/** A thumbnail of a page in `theme`, drawn with that theme's own tokens. */
function ThemePage({ theme }: { theme: Exclude<Theme, 'system'> }) {
  return (
    <span className="theme-page" data-theme={theme}>
      <span className="theme-page__line theme-page__line--title" />
      <span className="theme-page__line" />
      <span className="theme-page__line theme-page__line--short" />
      <span className="theme-page__accent" />
    </span>
  )
}

function WritingSection() {
  const { user, updateUser } = useAuth()
  const { busy, run, status } = useRun()
  const { preferences, update: setPreference } = useWritingPreferences()
  if (!user) return null
  return (
    <Section title="Writing">
      <label className="settings__row">
        <input
          type="checkbox"
          checked={!!user.doubleEnterSplitsScenes}
          disabled={busy}
          onChange={(e) => {
            const on = e.target.checked
            void run(async () => {
              updateUser(await api.account.updateSettings({ doubleEnterSplitsScenes: on }))
            })
          }}
        />
        Treat a blank line (double Enter) as a scene break
      </label>
      <p className="field__hint">
        While writing a book, in the writing room too, pressing Enter on an empty line moves on to the next scene, or starts one after the last. In the middle of a scene, it splits the scene there. Turning a Daily Prompts answer into a project splits it at blank lines as well as at lines that are only <code>--</code>. Kept with your account.
      </p>
      <label className="settings__row">
        <input
          type="checkbox"
          checked={preferences.spellcheck}
          onChange={(e) => setPreference('spellcheck', e.target.checked)}
        />
        Check spelling as I type
      </label>
      <p className="field__hint">
        Underlines misspelled words in books and Daily Prompts answers; right-click one for corrections. Kept on this device.
      </p>
      {status}
    </Section>
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
    : /JustType|Scriptorium|CFNetwork/.test(agent) ? 'The iOS app'
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

function AssistantSection() {
  const { user, updateUser } = useAuth()
  const [available, setAvailable] = useState(false)
  const { busy, run, status } = useRun()
  useEffect(() => {
    api.features().then((f) => setAvailable(f.assistant)).catch(() => undefined)
  }, [])
  if (!available || !user) return null
  const on = !!user.assistantAllowedAt
  return (
    <Section title="Writing assistant">
      <p className="field__hint">
        The writing assistant — continue from the cursor, rewrite or proofread a passage, ask
        about it — is Claude, a model Anthropic runs. When it is on, what you hand it is sent to
        Anthropic when you ask, and only then. What it writes goes into your manuscript only if
        you put it there. Spelling and the thesaurus never send your words anywhere.
      </p>
      <div className="settings__row">
        <span>{on ? `On since ${new Date(user.assistantAllowedAt!).toLocaleDateString()}` : 'Off'}</span>
        <button
          type="button"
          disabled={busy}
          onClick={() => run(async () => {
            updateUser(on ? await api.assistant.withdraw() : await api.assistant.allow())
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
  assistant_allowed: 'Writing assistant turned on',
  assistant_withdrawn: 'Writing assistant turned off',
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
