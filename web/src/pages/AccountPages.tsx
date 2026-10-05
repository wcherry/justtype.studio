import { useEffect, useRef, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { api } from '../api/client'
import type { Terms } from '../api/types'
import { useAuth } from '../state/auth'

/** The pages a mailed link lands on, and the ones a signed-out person needs. */

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

export function ForgotPage() {
  const [email, setEmail] = useState('')
  const [sent, setSent] = useState(false)
  const [error, setError] = useState<string | null>(null)

  if (sent) {
    return (
      <Card title="Check your mail">
        <p className="auth__tagline">
          If there is an account for {email}, a link to reset its password is on its way. It
          lasts an hour.
        </p>
        <Link to="/sign-in">Back to sign in</Link>
      </Card>
    )
  }
  return (
    <Card title="Forgot your password?">
      <p className="auth__tagline">We will mail you a link to set a new one.</p>
      <form
        onSubmit={async (event) => {
          event.preventDefault()
          setError(null)
          try {
            await api.account.forgot(email)
            setSent(true)
          } catch (e) {
            setError(e instanceof Error ? e.message : 'Could not send that')
          }
        }}
      >
        <label>
          Email
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required autoFocus />
        </label>
        {error && <p className="error">{error}</p>}
        <button type="submit">Send the link</button>
      </form>
      <Link to="/sign-in">Back to sign in</Link>
    </Card>
  )
}

export function ResetPage() {
  const [params] = useSearchParams()
  const token = params.get('token') ?? ''
  const [password, setPassword] = useState('')
  const [done, setDone] = useState(false)
  const [error, setError] = useState<string | null>(null)

  if (done) {
    return (
      <Card title="Password changed">
        <p className="auth__tagline">Every device that was signed in has been signed out.</p>
        <Link to="/sign-in">Sign in with the new one</Link>
      </Card>
    )
  }
  return (
    <Card title="A new password">
      <form
        onSubmit={async (event) => {
          event.preventDefault()
          setError(null)
          try {
            await api.account.reset(token, password)
            setDone(true)
          } catch (e) {
            setError(e instanceof Error ? e.message : 'Could not change it')
          }
        }}
      >
        <label>
          New password
          <input
            type="password"
            minLength={8}
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            autoFocus
          />
        </label>
        {error && <p className="error">{error}</p>}
        <button type="submit" disabled={!token}>Set it</button>
      </form>
      {!token && <p className="error">This page needs the link from the email.</p>}
    </Card>
  )
}

export function VerifyPage() {
  const [params] = useSearchParams()
  const token = params.get('token') ?? ''
  const { user, refreshUser } = useAuth()
  const [state, setState] = useState<'working' | 'done' | 'failed'>('working')
  const [error, setError] = useState<string | null>(null)
  // A link is spent on first use, so StrictMode's second effect must not
  // send it again and report the first success as a failure.
  const sent = useRef(false)

  useEffect(() => {
    if (sent.current) return
    sent.current = true
    api.account
      .verify(token)
      .then(async () => {
        setState('done')
        if (user) await refreshUser().catch(() => undefined)
      })
      .catch((e) => {
        setState('failed')
        setError(e instanceof Error ? e.message : 'That link did not work')
      })
  }, [token, user, refreshUser])

  return (
    <Card title={state === 'done' ? 'Address confirmed' : state === 'failed' ? 'That link did not work' : 'Confirming…'}>
      {state === 'done' && (
        <p className="auth__tagline">Drafts and books shared with this address will now reach you.</p>
      )}
      {error && <p className="error">{error}</p>}
      <Link to={user ? '/' : '/sign-in'}>{user ? 'To your desk' : 'Sign in'}</Link>
    </Card>
  )
}

export function TermsPage() {
  const [terms, setTerms] = useState<Terms | null>(null)
  useEffect(() => {
    api.trust.terms().then(setTerms).catch(() => undefined)
  }, [])
  return (
    <main className="feed terms">
      {terms ? <Markdown text={terms.text} /> : <p className="empty">Loading…</p>}
      {terms && <p className="field__hint">Version {terms.version}.</p>}
    </main>
  )
}

/**
 * Enough Markdown for the terms: headings, paragraphs and bullet lists. The
 * text is ours and checked in, so this renders it as React elements rather
 * than reaching for a parser and `innerHTML`.
 */
export function Markdown({ text }: { text: string }) {
  const blocks = text.trim().split(/\n\s*\n/)
  return (
    <>
      {blocks.map((block, i) => {
        const line = block.trim()
        if (line.startsWith('## ')) return <h2 key={i}>{line.slice(3)}</h2>
        if (line.startsWith('# ')) return <h1 key={i}>{line.slice(2)}</h1>
        if (line.split('\n').every((l) => l.trim().startsWith('- '))) {
          return (
            <ul key={i}>
              {line.split('\n').map((l, j) => <li key={j}>{l.trim().slice(2)}</li>)}
            </ul>
          )
        }
        return <p key={i}>{line.replace(/\s*\n\s*/g, ' ')}</p>
      })}
    </>
  )
}
