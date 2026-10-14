import { useState } from 'react'
import { Link } from 'react-router-dom'
import { api } from '../api/client'
import type { PublicationScope, ReviewView } from '../api/types'

const KIND: Record<PublicationScope, string> = {
  work: 'Book',
  part: 'Part',
  chapter: 'Chapter',
  scene: 'Scene',
}

/** Who a review is for, in a few words. */
function audienceOf(review: ReviewView): string {
  switch (review.visibility) {
    case 'platform':
      return 'Public'
    case 'public':
      return 'Open to all, signed in or not'
    case 'unlisted':
      return 'Anyone with the link'
    default: {
      const names = review.people.map((p) => p.user?.displayName ?? p.email ?? '')
      if (names.length <= 2) return names.join(' and ') || 'Nobody yet'
      return `${names.slice(0, 2).join(', ')} and ${names.length - 2} more`
    }
  }
}

const STATE_LABEL: Record<ReviewView['state'], string> = {
  open: 'Open',
  closed: 'Closed',
  revoked: 'Revoked',
}

/**
 * The book's reviews: what each is of, who it is for, the version readers
 * see, and whether the draft has moved on since. Each can be brought up to
 * date, changed, revoked or opened again.
 */
export function ReviewsPanel({
  reviews, canManage, onChanged, onEdit,
}: {
  reviews: ReviewView[]
  /** Whether this writer may manage the review: an owner, or a writer handed what it covers. */
  canManage: (review: ReviewView) => boolean
  /** A review as it now stands, after an action here. */
  onChanged: (review: ReviewView) => void
  onEdit: (review: ReviewView) => void
}) {
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showPast, setShowPast] = useState(false)

  const act = async (review: ReviewView, work: () => Promise<ReviewView>) => {
    setBusy(review.id)
    setError(null)
    try {
      onChanged(await work())
    } catch (e) {
      setError(e instanceof Error ? e.message : 'That did not work')
    } finally {
      setBusy(null)
    }
  }

  const open = reviews.filter((r) => r.state === 'open')
  const past = reviews.filter((r) => r.state !== 'open')
  const shown = showPast ? [...open, ...past] : open

  return (
    <div className="reviews">
      {error && <p className="error">{error}</p>}
      {reviews.length === 0 && (
        <p className="empty">
          Nothing is up for review. Right-click a scene, a chapter or a part in the outline and choose
          “Mark for review…”, or use Book → Mark for review… for the whole book.
        </p>
      )}
      {reviews.length > 0 && open.length === 0 && !showPast && <p className="empty">No review is open right now.</p>}

      <ul className="reviews__list">
        {shown.map((review) => {
          const manage = canManage(review)
          const working = busy === review.id
          return (
            <li key={review.id} className={`reviews__item is-${review.state}`}>
              <header className="reviews__head">
                <span className="reviews__kind">{KIND[review.scope]}</span>
                <span className="reviews__title">{review.title}</span>
                <span className={`reviews__state reviews__state--${review.state}`}>{STATE_LABEL[review.state]}</span>
              </header>
              <p className="reviews__meta">
                {audienceOf(review)} · {review.versionLabel}
                {review.closesAt && review.state === 'open' && <> · closes {new Date(review.closesAt).toLocaleDateString()}</>}
              </p>
              <p className="reviews__meta">
                {review.commentCount} {review.commentCount === 1 ? 'note' : 'notes'} · {review.readerCount}{' '}
                {review.readerCount === 1 ? 'reader' : 'readers'}
              </p>
              {review.outdated && review.state === 'open' && (
                <p className="reviews__outdated">The draft has changed since {review.versionLabel}.</p>
              )}
              <div className="reviews__actions">
                <Link to={`/review/${review.id}`}>Open</Link>
                {manage && review.state === 'open' && review.outdated && (
                  <button type="button" disabled={working} onClick={() => void act(review, () => api.reviews.bringUpToDate(review.id))}>
                    Update to latest
                  </button>
                )}
                {manage && (review.visibility === 'platform' || review.visibility === 'invited') && (
                  <button type="button" disabled={working} onClick={() => onEdit(review)}>Change…</button>
                )}
                {manage && review.state === 'open' && (
                  <button type="button" disabled={working} onClick={() => void act(review, () => api.reviews.revoke(review.id))}>
                    Revoke
                  </button>
                )}
                {manage && review.state !== 'open' && (
                  <button type="button" disabled={working} onClick={() => void act(review, () => api.reviews.reopen(review.id))}>
                    Open again
                  </button>
                )}
              </div>
            </li>
          )
        })}
      </ul>

      {past.length > 0 && (
        <button type="button" className="linklike" onClick={() => setShowPast(!showPast)}>
          {showPast ? 'Hide closed and revoked' : `Show ${past.length} closed or revoked`}
        </button>
      )}
      <p className="field__hint">
        Revoking closes a review to its readers; you keep every note, and can open it again. Notes left on an earlier
        version stay with the review and say which version they were written on.
      </p>
    </div>
  )
}
