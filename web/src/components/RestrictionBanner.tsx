import { useState } from 'react'
import { Link } from 'react-router-dom'
import { downloadAccountExport } from '../api/client'
import { longDate, useStanding } from '../state/standing'

/**
 * Shown on every page to a writer whose account is restricted: what they can't
 * do, until when, why, and — for an account that will be deleted — the date,
 * the days left, and the button that takes their work away first.
 */
export function RestrictionBanner() {
  const { standing } = useStanding()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  if (!standing || (standing.active.length === 0 && !standing.closing)) return null
  const closing = standing.closing
  return (
    <div className="banner banner--restriction" role="status">
      {standing.active.map((r) => (
        <p key={r.id}>
          {r.scope === 'discussion'
            ? "You can't take part in discussions"
            : r.kind === 'read_only'
              ? "Your account is read-only: you can read and export, but not write"
              : r.kind === 'suspend'
                ? "You can't publish or take part in discussions"
                : 'This account is restricted'}
          {r.endsAt ? ` until ${longDate(r.endsAt)}` : ''}.{' '}
          <Link to="/standing">Why?</Link>
        </p>
      ))}
      {closing && (
        <p className="banner__closing">
          <strong>
            This account and everything in it will be deleted on {longDate(closing.purgeAt)}
            {' '}({closing.daysLeft} day{closing.daysLeft === 1 ? '' : 's'} left).
          </strong>{' '}
          {closing.appealOpen && 'Your appeal is waiting for a decision; nothing is deleted until it has one. '}
          <button
            type="button"
            className="linklike"
            disabled={busy}
            onClick={() => {
              setBusy(true)
              setError(null)
              downloadAccountExport()
                .catch((e) => setError(e instanceof Error ? e.message : 'Could not export'))
                .finally(() => setBusy(false))
            }}
          >
            Export my work
          </button>
          {error && <span className="error"> {error}</span>}
        </p>
      )}
    </div>
  )
}
