import { useEffect, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router-dom'
import { ApiError, standing as standingApi, tokens } from '../api/client'
import type { MyRestriction } from '../api/types'
import { useAuth } from '../state/auth'
import { longDate, useStanding } from '../state/standing'

const KIND: Record<string, string> = {
  warning: 'Warning',
  ban: 'Ban',
  suspend: 'Suspension',
  read_only: 'Read-only',
}

function describe(r: MyRestriction): string {
  const where = r.scope === 'discussion' ? 'in discussions' : 'on the whole site'
  const until = r.endsAt ? `until ${longDate(r.endsAt)}` : 'with no end date'
  return r.kind === 'warning' ? 'A warning, with no effect on what you can do' : `${KIND[r.kind]} ${where}, ${until}`
}

function Appeal({ restriction, onFiled, send }: {
  restriction: MyRestriction
  onFiled: () => void
  send: (body: string) => Promise<unknown>
}) {
  const [body, setBody] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  if (restriction.appeal) {
    return (
      <p className="field__hint">
        You appealed on {longDate(restriction.appeal.createdAt)}:{' '}
        {restriction.appeal.state === 'open' && 'waiting for a decision.'}
        {restriction.appeal.state === 'upheld' && 'the restriction stands.'}
        {restriction.appeal.state === 'overturned' && 'the restriction was lifted.'}
        {restriction.appeal.decisionText && <> “{restriction.appeal.decisionText}”</>}
      </p>
    )
  }
  if (!restriction.inForce) return null
  return (
    <form
      className="standing__appeal"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        setError(null)
        try {
          await send(body)
          onFiled()
        } catch (e) {
          setError(e instanceof Error ? e.message : 'Could not send your appeal')
        } finally {
          setBusy(false)
        }
      }}
    >
      <label>
        Appeal this once. Say why you think it was a mistake.
        <textarea rows={4} maxLength={2000} value={body} onChange={(e) => setBody(e.target.value)} required />
      </label>
      <span className="field__hint">{body.length}/2000. A different member of staff will read it.</span>
      {error && <p className="error">{error}</p>}
      <button type="submit" disabled={busy || !body.trim()}>Send appeal</button>
    </form>
  )
}

function RestrictionCard({ r, children }: { r: MyRestriction; children?: React.ReactNode }) {
  return (
    <li className="standing__item">
      <h3>{describe(r)}</h3>
      <p>
        <strong>Community guidelines, section {r.reasonCode}.</strong> {r.reasonText}
      </p>
      {r.closing && r.inForce && (
        <p className="error">
          This account, all of its writing and all of its comments will be deleted 30 days after the
          restriction began.
        </p>
      )}
      {children}
    </li>
  )
}

/** Where the signed-in writer's account stands, and the one appeal each restriction allows. */
export function StandingPage() {
  const { standing, refresh } = useStanding()
  if (!standing) return <main className="empty">Loading…</main>
  return (
    <main className="feed standing">
      <header className="feed__header"><h1>Your account</h1></header>
      {standing.active.length === 0 && standing.history.length === 0 && (
        <p className="empty">Nothing is restricted, and nothing has been. Thank you for keeping it kind here.</p>
      )}
      {standing.active.length > 0 && (
        <section>
          <h2>In force now</h2>
          <ul className="standing__list">
            {standing.active.map((r) => (
              <RestrictionCard key={r.id} r={r}>
                <Appeal restriction={r} onFiled={refresh} send={(body) => standingApi.appeal(r.id, body)} />
              </RestrictionCard>
            ))}
          </ul>
        </section>
      )}
      {standing.history.length > 0 && (
        <section>
          <h2>Before</h2>
          <ul className="standing__list">
            {standing.history.map((r) => (
              <RestrictionCard key={r.id} r={r}>
                {r.lifted && <p className="field__hint">Lifted.</p>}
                <Appeal restriction={r} onFiled={refresh} send={(body) => standingApi.appeal(r.id, body)} />
              </RestrictionCard>
            ))}
          </ul>
        </section>
      )}
      <p><Link to="/">Back to your desk</Link></p>
    </main>
  )
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <main className="auth">
      <div className="auth__card">
        <h1>{title}</h1>
        {children}
      </div>
    </main>
  )
}

/** A banned writer, from the link in the email: what happened, and a way to disagree. */
export function BanAppealPage() {
  const [params] = useSearchParams()
  const token = params.get('token') ?? ''
  const [restriction, setRestriction] = useState<MyRestriction | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [filed, setFiled] = useState(false)
  useEffect(() => {
    standingApi.peek(token).then(setRestriction).catch((e) => setError(e instanceof Error ? e.message : 'That link does not work'))
  }, [token])
  if (error) return <Card title="That link does not work"><p className="error">{error}</p><Link to="/sign-in">Back to sign in</Link></Card>
  if (!restriction) return <Card title="One moment"><p className="auth__tagline">Loading…</p></Card>
  return (
    <Card title="Your appeal">
      <ul className="standing__list"><RestrictionCard r={restriction} /></ul>
      {filed ? (
        <p className="auth__tagline">Your appeal has been sent. You will get an email with the decision.</p>
      ) : (
        <Appeal restriction={{ ...restriction, appeal: null }} onFiled={() => setFiled(true)} send={(body) => standingApi.appealFromLink(token, body)} />
      )}
      {restriction.closing && <p><Link to="/ban/export">Have a copy of my work emailed to me</Link></p>}
      <Link to="/sign-in">Back to sign in</Link>
    </Card>
  )
}

/** For someone who can't sign in: a copy of their work, mailed to the account's address. */
export function BanExportPage() {
  const [email, setEmail] = useState('')
  const [sent, setSent] = useState(false)
  const [error, setError] = useState<string | null>(null)
  if (sent) {
    return (
      <Card title="Check your mail">
        <p className="auth__tagline">
          If {email} is an account that is waiting to be deleted, a link to a copy of everything in it is on its
          way. It works for 7 days, without signing in.
        </p>
        <Link to="/sign-in">Back to sign in</Link>
      </Card>
    )
  }
  return (
    <Card title="Get a copy of your work">
      <p className="auth__tagline">
        While an account is waiting to be deleted, we can email its writer a copy of everything in it.
      </p>
      <form
        onSubmit={async (event) => {
          event.preventDefault()
          setError(null)
          try {
            await standingApi.emailMyWork(email)
            setSent(true)
          } catch (e) {
            setError(e instanceof ApiError ? e.message : 'Could not send that')
          }
        }}
      >
        <label>
          The account's email
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required autoFocus />
        </label>
        {error && <p className="error">{error}</p>}
        <button type="submit">Email me a copy</button>
      </form>
      <Link to="/sign-in">Back to sign in</Link>
    </Card>
  )
}

/** The link in the email a staff-made account is sent: choose a password and agree to the terms. */
export function SetupPage() {
  const [params] = useSearchParams()
  const token = params.get('token') ?? ''
  const navigate = useNavigate()
  const { refreshUser } = useAuth()
  const [password, setPassword] = useState('')
  const [accept, setAccept] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  return (
    <Card title="Set up your account">
      <p className="auth__tagline">Choose a password to finish. Your writing stays yours.</p>
      <form
        onSubmit={async (event) => {
          event.preventDefault()
          setBusy(true)
          setError(null)
          try {
            const session = await standingApi.setup(token, password, accept)
            tokens.set(session)
            await refreshUser()
            navigate('/', { replace: true })
          } catch (e) {
            setError(e instanceof Error ? e.message : 'Could not set up your account')
          } finally {
            setBusy(false)
          }
        }}
      >
        <label>
          Password
          <input type="password" minLength={8} value={password} onChange={(e) => setPassword(e.target.value)} required autoFocus />
        </label>
        <label className="auth__check">
          <input type="checkbox" checked={accept} onChange={(e) => setAccept(e.target.checked)} />
          <span>I agree to the <Link to="/terms" target="_blank">terms</Link>.</span>
        </label>
        {error && <p className="error">{error}</p>}
        <button type="submit" disabled={busy || !accept || password.length < 8}>Finish</button>
      </form>
    </Card>
  )
}
