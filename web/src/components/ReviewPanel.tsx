import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { CircleView, InviteView, PublicationCard, ReaderProgressView } from '../api/types'

interface Props {
  publication: PublicationCard
  onChanged: () => void
}

/**
 * Who a manuscript went to, and how far they got.
 *
 * The progress list is the useful half: "three people opened it and all three
 * stopped in chapter two" is worth more than any single comment, and it is the
 * one signal readers never give you voluntarily.
 */
export function ReviewPanel({ publication, onChanged }: Props) {
  const [invites, setInvites] = useState<InviteView[]>([])
  const [progress, setProgress] = useState<ReaderProgressView[]>([])
  const [circles, setCircles] = useState<CircleView[]>([])
  // Both lists claim something about the server ("nobody yet"), so neither is
  // said until the server has answered.
  const [loaded, setLoaded] = useState(false)
  const [emails, setEmails] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    void Promise.all([
      api.publications.invites(publication.id).then(setInvites).catch(() => setInvites([])),
      api.publications.progress(publication.id).then(setProgress).catch(() => setProgress([])),
      api.circles.list().then(setCircles).catch(() => setCircles([])),
    ]).finally(() => setLoaded(true))
  }, [publication.id])

  useEffect(load, [load])

  const run = async (work: () => Promise<unknown>) => {
    setBusy(true)
    setError(null)
    try {
      await work()
      load()
      onChanged()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not do that')
    } finally {
      setBusy(false)
    }
  }

  const invited = publication.visibility === 'invited'
  const unlisted = publication.visibility === 'unlisted'
  const [days, setDays] = useState(14)
  const [link, setLink] = useState<{ url: string; expiresAt: string } | null>(null)

  return (
    <div className="review-panel">
      {error && <p className="error">{error}</p>}

      {unlisted && (
        <section>
          <h4>Links</h4>
          <p className="field__hint">
            An unlisted draft opens only from a link you make here. Each one stops working when
            it expires, and you can withdraw them all at once.
          </p>
          <div className="settings__row">
            <select value={days} onChange={(e) => setDays(Number(e.target.value))}>
              {[1, 7, 14, 30, 90].map((d) => (
                <option key={d} value={d}>{d === 1 ? 'for a day' : `for ${d} days`}</option>
              ))}
            </select>
            <button
              type="button"
              disabled={busy}
              onClick={() => run(async () => setLink(await api.publications.makeLink(publication.id, days)))}
            >
              Make a link
            </button>
            <button
              type="button"
              className="linklike"
              disabled={busy}
              onClick={() => run(async () => {
                await api.publications.revokeLinks(publication.id)
                setLink(null)
              })}
            >
              Withdraw every link
            </button>
          </div>
          {link && (
            <p className="share-link">
              <input readOnly value={link.url} onFocus={(e) => e.currentTarget.select()} />
              <span className="field__hint">
                Works until {new Date(link.expiresAt).toLocaleString()}.
              </span>
            </p>
          )}
        </section>
      )}

      <section>
        <h4>Readers</h4>
        {!invited && (
          <p className="field__hint">
            This is {publication.visibility === 'public'
              ? 'open to all, signed in or not'
              : unlisted ? 'open to anyone with the link' : 'public: anyone signed in can read it'}.
            Invitations only gate a publication set to named readers.
          </p>
        )}

        <form
          onSubmit={(event) => {
            event.preventDefault()
            const list = emails.split(/[,\s]+/).filter(Boolean)
            if (list.length === 0) return
            void run(async () => {
              await api.publications.invite(publication.id, { emails: list })
              setEmails('')
            })
          }}
        >
          <label>
            Invite by email
            <input
              value={emails}
              placeholder="one@example.com, two@example.com"
              onChange={(e) => setEmails(e.target.value)}
            />
          </label>
          <button type="submit" disabled={busy || !emails.trim()}>Invite</button>
        </form>

        {circles.length > 0 && (
          <div className="review-panel__circles">
            {circles.map((circle) => (
              <button
                key={circle.id}
                type="button"
                disabled={busy}
                onClick={() => run(() => api.publications.invite(publication.id, { circleId: circle.id }))}
              >
                Invite {circle.name} ({circle.members.length})
              </button>
            ))}
          </div>
        )}

        <ul className="review-panel__list">
          {invites.map((invite) => (
            <li key={invite.id}>
              <span className={invite.accepted ? 'is-accepted' : undefined}>{invite.email}</span>
              <span className="field__hint">{invite.accepted ? 'opened it' : 'not yet'}</span>
              <button
                type="button"
                disabled={busy}
                onClick={() => run(() => api.publications.uninvite(publication.id, invite.email))}
              >
                ×
              </button>
            </li>
          ))}
          {loaded && invites.length === 0 && <li className="empty">Nobody invited yet.</li>}
        </ul>
      </section>

      <section>
        <h4>How far they got</h4>
        {loaded && progress.length === 0 && <p className="empty">Nobody has opened it yet.</p>}
        <ul className="review-panel__progress">
          {progress.map((reader) => (
            <li key={reader.id}>
              <div className="review-panel__reader">
                <span>{reader.reader.displayName}</span>
                <span className="field__hint">
                  {reader.finishedAt ? 'finished' : `${reader.percent}%`}
                </span>
              </div>
              <div className="review-panel__bar">
                <div
                  className={reader.finishedAt ? 'is-finished' : undefined}
                  style={{ width: `${reader.percent}%` }}
                />
              </div>
            </li>
          ))}
        </ul>
      </section>
    </div>
  )
}
