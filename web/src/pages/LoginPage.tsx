import { useState } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { ApiError } from '../api/client'
import { useAuth } from '../state/auth'

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
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [acceptTerms, setAcceptTerms] = useState(false)
  /** Shown once the password was right and the account wants a second factor. */
  const [needsCode, setNeedsCode] = useState(false)
  const [code, setCode] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      if (mode === 'login') await login(email, password, needsCode ? code : undefined)
      else await register(email, password, displayName, acceptTerms)
      navigate(back, { replace: true })
    } catch (e) {
      if (e instanceof ApiError && e.code === 'two_factor_required') {
        setNeedsCode(true)
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
          {error && <p className="error">{error}</p>}
          <button type="submit" disabled={busy || (mode === 'register' && !acceptTerms)}>
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
