import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { api } from '../api/client'
import { CircleManager } from '../components/CircleManager'
import { FeedbackPanel } from '../components/FeedbackPanel'
import { ReviewPanel } from '../components/ReviewPanel'
import type { PublicationCard } from '../api/types'

export function FeedPage() {
  const [cards, setCards] = useState<PublicationCard[]>([])
  const [mine, setMine] = useState(false)
  const [managing, setManaging] = useState<string | null>(null)
  const [reading, setReading] = useState<string | null>(null)
  const [showCircles, setShowCircles] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.publications
      .feed({ mine })
      .then(setCards)
      .catch((e) => setError(e.message))
  }, [mine])

  useEffect(() => {
    load()
  }, [load])

  return (
    <main className="feed">
      <header className="feed__header">
        <h1>{mine ? 'Your publications' : 'Looking for readers'}</h1>
        <div className="feed__actions">
          <button type="button" onClick={() => setShowCircles((c) => !c)}>
            {showCircles ? 'Hide circles' : 'Reader circles'}
          </button>
          <button type="button" onClick={() => setMine((m) => !m)}>
            {mine ? 'Show everyone’s' : 'Show only mine'}
          </button>
        </div>
      </header>

      {showCircles && (
        <section className="feed__circles">
          <h2>Reader circles</h2>
          <p className="field__hint">
            Standing lists of people you send drafts to, so inviting them is one action
            rather than eleven.
          </p>
          <CircleManager />
        </section>
      )}

      {error && <p className="error">{error}</p>}
      {cards.length === 0 && <p className="empty">Nothing on offer right now.</p>}

      <ul className="feed__list">
        {cards.map((card) => (
          <li key={card.id}>
            <Link to={`/review/${card.id}`} className="feed__card">
              <h2>{card.workTitle}</h2>
              <p className="feed__meta">
                {card.author.displayName} · {card.workKind.replace('_', ' ')} ·{' '}
                {card.wordCount.toLocaleString()} words · {card.versionLabel}
                <span className="feed__count">
                  {card.commentCount} comment{card.commentCount === 1 ? '' : 's'}
                </span>
              </p>
              {card.reviewPrompt && <p className="feed__prompt">“{card.reviewPrompt}”</p>}
              {card.excerpt && <p className="feed__excerpt">{card.excerpt}</p>}
            </Link>
            {mine && (
              <>
                <div className="feed__manage">
                  <button
                    type="button"
                    className="linklike"
                    onClick={() => setManaging(managing === card.id ? null : card.id)}
                  >
                    {managing === card.id ? 'Hide readers' : 'Readers and progress'}
                  </button>
                  <button
                    type="button"
                    className="linklike"
                    onClick={() => setReading(reading === card.id ? null : card.id)}
                  >
                    {reading === card.id ? 'Hide feedback' : 'Reactions and questions'}
                  </button>
                </div>
                {managing === card.id && <ReviewPanel publication={card} onChanged={load} />}
                {reading === card.id && <FeedbackPanel publication={card} />}
              </>
            )}
          </li>
        ))}
      </ul>
    </main>
  )
}
