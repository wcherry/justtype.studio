import { useEffect, useRef, useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { ApiError, api } from '../api/client'
import { TurnstileWidget, type TurnstileHandle } from '../components/TurnstileWidget'
import { useAuth } from '../state/auth'

interface BanDetails {
  until?: string | null
  reason?: string
  reasonCode?: string
}

/** Why a banned account can't come in — shown in place of a generic error. */
function BanNotice({ ban }: { ban: BanDetails }) {
  return (
    <div className="banner banner--restriction" role="alert">
      <p>
        <strong>This account is banned{ban.until ? ` until ${new Date(ban.until).toLocaleDateString()}` : ''}.</strong>
      </p>
      {ban.reason && (
        <p>
          Community guidelines, section {ban.reasonCode}: {ban.reason}
        </p>
      )}
      <p>
        The email we sent has a link to appeal. If it will be deleted, you can also{' '}
        <Link to="/ban/export">have a copy of your work emailed to you</Link>.
      </p>
    </div>
  )
}

export function LoginPage() {
  const { login, register } = useAuth()
  const navigate = useNavigate()
  // Sent here from somewhere (the link in the prompt email, say): go back there.
  // Never into a book, though: the page that sent us here may have been the
  // last writer's, left open when their session ran out. Signing in lands on
  // the desk, and opening a book from there picks up where it was left.
  const location = useLocation()
  const from = (location.state as { from?: { pathname: string; search?: string; hash?: string } } | null)?.from
  const back = from && !from.pathname.startsWith('/works/')
    ? `${from.pathname}${from.search ?? ''}${from.hash ?? ''}`
    : '/'
  // Someone who followed an invitation has no account yet: open on sign-up.
  const invited = new URLSearchParams(location.search).has('invited')
  const [mode, setMode] = useState<'login' | 'register'>(invited ? 'register' : 'login')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [acceptTerms, setAcceptTerms] = useState(false)
  /** Shown once the password was right and the account wants a second factor. */
  const [needsCode, setNeedsCode] = useState(false)
  const [code, setCode] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [ban, setBan] = useState<BanDetails | null>(null)
  const [busy, setBusy] = useState(false)
  /** The Turnstile site key, when sign-up asks for the check. */
  const [siteKey, setSiteKey] = useState<string | null>(null)
  const [captchaToken, setCaptchaToken] = useState<string | null>(null)
  /** The check could not load: sign up without it, under the tighter limits. */
  const [captchaUnavailable, setCaptchaUnavailable] = useState(false)
  const captcha = useRef<TurnstileHandle>(null)

  useEffect(() => {
    if (mode !== 'register') return
    let gone = false
    api.auth.signupPolicy()
      .then((policy) => { if (!gone) setSiteKey(policy.turnstileSiteKey ?? null) })
      // An older server, or none: sign up without the check.
      .catch(() => undefined)
    return () => { gone = true }
  }, [mode])

  const waitingOnCaptcha = mode === 'register' && !!siteKey && !captchaToken && !captchaUnavailable

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    setBan(null)
    try {
      if (mode === 'login') await login(email, password, needsCode ? code : undefined)
      else await register(email, password, displayName, acceptTerms, captchaToken ?? undefined)
      navigate(back, { replace: true })
    } catch (e) {
      // A token is good for one try, whatever the answer was.
      if (mode === 'register') captcha.current?.reset()
      if (e instanceof ApiError && e.code === 'two_factor_required') {
        setNeedsCode(true)
      } else if (e instanceof ApiError && e.code === 'account_banned') {
        setBan(e.details as BanDetails)
      } else {
        setError(e instanceof Error ? e.message : 'Could not sign you in')
      }
    } finally {
      setBusy(false)
    }
  }

  return (
    <main className="auth">
      <div className="auth__card">
        <h1>Just Type Studio</h1>
        <p className="auth__tagline">Write it. Version it. Put it in front of readers.</p>

        <form onSubmit={submit}>
          {mode === 'register' && (
            <label>
              Display name
              <input value={displayName} onChange={(e) => setDisplayName(e.target.value)} required />
            </label>
          )}
          <label>
            Email
            <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
          </label>
          <label>
            Password
            <input
              type="password"
              value={password}
              minLength={8}
              onChange={(e) => setPassword(e.target.value)}
              required
            />
          </label>
          {needsCode && mode === 'login' && (
            <label>
              Code from your authenticator
              <input
                value={code}
                inputMode="numeric"
                autoComplete="one-time-code"
                autoFocus
                onChange={(e) => setCode(e.target.value)}
                required
              />
              <span className="field__hint">Or one of your recovery codes.</span>
            </label>
          )}
          {mode === 'register' && (
            <label className="auth__check">
              <input
                type="checkbox"
                checked={acceptTerms}
                onChange={(e) => setAcceptTerms(e.target.checked)}
              />
              <span>
                I agree to the <Link to="/terms" target="_blank">terms</Link> — which say my
                writing stays mine.
              </span>
            </label>
          )}
          {mode === 'register' && siteKey && (
            <TurnstileWidget
              ref={captcha}
              siteKey={siteKey}
              onToken={setCaptchaToken}
              onUnavailable={() => setCaptchaUnavailable(true)}
            />
          )}
          {ban && <BanNotice ban={ban} />}
          {error && <p className="error">{error}</p>}
          <button type="submit" disabled={busy || (mode === 'register' && (!acceptTerms || waitingOnCaptcha))}>
            {mode === 'login' ? 'Sign in' : 'Create account'}
          </button>
        </form>

        <div className="auth__links">
          <button
            type="button"
            className="linklike"
            onClick={() => {
              setMode(mode === 'login' ? 'register' : 'login')
              setError(null)
              setNeedsCode(false)
            }}
          >
            {mode === 'login' ? 'Need an account?' : 'Already have an account?'}
          </button>
          {mode === 'login' && <Link to="/forgot">Forgot your password?</Link>}
        </div>
      </div>
    </main>
  )
}
