import { useEffect, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { api, prompts } from '../api/client'

/**
 * The footer link in the prompt email. One click, signed in or not: arriving
 * here turns the email off, and nothing else.
 */
export function UnsubscribePage() {
  const [params] = useSearchParams()
  const token = params.get('token') ?? ''
  const [state, setState] = useState<'working' | 'done' | 'failed'>('working')

  useEffect(() => {
    if (!token) {
      setState('failed')
      return
    }
    prompts.unsubscribe(token).then(() => setState('done')).catch(() => setState('failed'))
  }, [token])

  return (
    <main className="auth">
      <div className="auth__card">
        <h1>Daily Prompts</h1>
        {state === 'working' && <p>Turning the email off…</p>}
        {state === 'done' && (
          <>
            <p>The daily email is off. You are still in Daily Prompts, and can turn it back on from its page.</p>
            <p><Link to="/prompts">Go to Daily Prompts</Link></p>
          </>
        )}
        {state === 'failed' && <p className="error">That link did not work. You can turn the email off from the Daily Prompts page.</p>}
      </div>
    </main>
  )
}

/**
 * The footer link in the summary email. One click, signed in or not: arriving
 * here turns the summary off, and nothing else.
 */
export function SummaryUnsubscribePage() {
  const [params] = useSearchParams()
  const token = params.get('token') ?? ''
  const [state, setState] = useState<'working' | 'done' | 'failed'>('working')

  useEffect(() => {
    if (!token) {
      setState('failed')
      return
    }
    api.account.summaryUnsubscribe(token).then(() => setState('done')).catch(() => setState('failed'))
  }, [token])

  return (
    <main className="auth">
      <div className="auth__card">
        <h1>Summary email</h1>
        {state === 'working' && <p>Turning the summary off…</p>}
        {state === 'done' && (
          <>
            <p>The summary email is off. You can turn it back on in your settings.</p>
            <p><Link to="/settings/general#summary-email">Go to settings</Link></p>
          </>
        )}
        {state === 'failed' && <p className="error">That link did not work. You can turn the summary off in your settings.</p>}
      </div>
    </main>
  )
}
