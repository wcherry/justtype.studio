import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import { useAuth } from '../state/auth'

/**
 * Beside the bell: ask someone to come and write here. The server sends the
 * mail, in its own words, so all a writer gives it is addresses.
 *
 * The reply is the same for an address that already has an account as for one
 * that does not, so this never says "already a member" — it cannot know.
 */
export function InviteButton() {
  const { user } = useAuth()
  const [open, setOpen] = useState(false)
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState<{ invited: string[]; remainingToday: number } | null>(null)
  const root = useRef<HTMLDivElement>(null)
  const field = useRef<HTMLTextAreaElement>(null)

  useEffect(() => {
    if (!open) return
    field.current?.focus()
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    const onDown = (event: MouseEvent) => {
      if (root.current && !root.current.contains(event.target as Node)) setOpen(false)
    }
    window.addEventListener('keydown', onKey)
    window.addEventListener('mousedown', onDown)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('mousedown', onDown)
    }
  }, [open])

  if (!user) return null
  const confirmed = !!user.emailVerifiedAt
  const emails = text.split(/[\s,;]+/).filter(Boolean)

  const send = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      setDone(await api.account.invite(emails))
      setText('')
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not send the invitation')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="invite" ref={root}>
      <button
        type="button"
        className="shell__bell invite__button"
        aria-label="Invite someone"
        aria-expanded={open}
        aria-haspopup="dialog"
        title="Invite someone"
        onClick={() => {
          setOpen(!open)
          setDone(null)
          setError(null)
        }}
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M15 19a6 6 0 0 0-12 0M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8M19 8v6M22 11h-6" />
        </svg>
      </button>
      {open && (
        <div className="invite__panel" role="dialog" aria-label="Invite someone">
          <h2>Invite someone to write here</h2>
          {!confirmed ? (
            <p className="field__hint">Confirm your own address first; the link is in the banner above.</p>
          ) : (
            <form onSubmit={send}>
              <label>
                Their email
                <textarea
                  ref={field}
                  rows={2}
                  value={text}
                  placeholder="friend@example.com"
                  onChange={(e) => setText(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey && emails.length > 0) {
                      e.preventDefault()
                      e.currentTarget.form?.requestSubmit()
                    }
                  }}
                />
              </label>
              <p className="field__hint">
                Several at once? Separate them with commas. We send a short note from you
                with a link to sign up, and only ever once per address.
              </p>
              {error && <p className="error" role="alert">{error}</p>}
              {done && (
                <p className="invite__done" role="status">
                  Sent to {done.invited.join(', ')}. You can invite {done.remainingToday} more today.
                </p>
              )}
              <button type="submit" disabled={busy || emails.length === 0}>
                {busy ? 'Sending…' : emails.length > 1 ? `Send ${emails.length} invitations` : 'Send invitation'}
              </button>
            </form>
          )}
        </div>
      )}
    </div>
  )
}
