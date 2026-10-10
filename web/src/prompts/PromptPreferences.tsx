import { useMemo, useState } from 'react'
import { api, prompts } from '../api/client'
import type { EnrollmentView } from '../api/types'
import { closingNote, timezones } from './days'

/**
 * Taking part: the email, when it comes, and leaving. The email needs a
 * confirmed address; until then it cannot be turned on, and the banner at the
 * top of every page is where to confirm it.
 */
export function PromptPreferences({
  enrollment,
  onChange,
}: {
  enrollment: EnrollmentView
  onChange: (next: EnrollmentView) => void
}) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [sent, setSent] = useState(false)
  const zones = useMemo(() => timezones(enrollment.emailTimezone), [enrollment.emailTimezone])

  const save = async (work: () => Promise<EnrollmentView>) => {
    setBusy(true)
    setError(null)
    try {
      onChange(await work())
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save that')
    } finally {
      setBusy(false)
    }
  }

  const note = enrollment.emailEnabled ? closingNote(enrollment.opensAtLocal, enrollment.emailLocalTime) : null

  return (
    <section className="prompts__section prompts__prefs" id="preferences" aria-labelledby="prompt-prefs">
      <h2 id="prompt-prefs">Preferences</h2>
      <label className="settings__row">
        <input
          type="checkbox"
          checked={enrollment.emailEnabled}
          disabled={busy || (!enrollment.emailVerified && !enrollment.emailEnabled)}
          onChange={(e) => void save(() => prompts.updateEnrollment({ emailEnabled: e.target.checked }))}
        />
        Email me the prompt each day
      </label>
      {!enrollment.emailVerified && (
        <p className="field__hint">
          Confirm your address first; the banner at the top of the page has the link.{' '}
          {sent ? 'Sent; check your mail.' : (
            <button
              type="button"
              className="linklike"
              onClick={() => api.account.resendVerification().finally(() => setSent(true))}
            >
              Send it again
            </button>
          )}
        </p>
      )}
      <div className="prompts__time">
        <label>
          At
          <input
            type="time"
            value={enrollment.emailLocalTime}
            disabled={busy}
            onChange={(e) => e.target.value && void save(() => prompts.updateEnrollment({ emailLocalTime: e.target.value }))}
          />
        </label>
        <label>
          Your timezone
          <select
            value={enrollment.emailTimezone}
            disabled={busy}
            onChange={(e) => void save(() => prompts.updateEnrollment({ emailTimezone: e.target.value }))}
          >
            {zones.map((z) => <option key={z} value={z}>{z.replace(/_/g, ' ')}</option>)}
          </select>
        </label>
      </div>
      <p className="field__hint">
        Each prompt opens at {enrollment.opensAtLocal} your time and stays open for a day.
        {note && <> {note}</>}
      </p>
      {error && <p className="error">{error}</p>}
      <button
        type="button"
        className="danger"
        disabled={busy}
        onClick={() => {
          if (window.confirm('Leave Daily Prompts? Your submitted answers and critiques stay for the others, and the email stops.')) {
            void save(() => prompts.leave())
          }
        }}
      >
        Leave Daily Prompts
      </button>
    </section>
  )
}
