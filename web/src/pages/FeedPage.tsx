import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { api } from '../api/client'
import type { PublicationCard } from '../api/types'

export function FeedPage() {
  const [cards, setCards] = useState<PublicationCard[]>([])
  const [mine, setMine] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api.publications
      .feed({ mine })
      .then(setCards)
      .catch((e) => setError(e.message))
  }, [mine])

  return (
    <main className="feed">
      <header className="feed__header">
        <h1>{mine ? 'Your publications' : 'Looking for readers'}</h1>
        <button type="button" onClick={() => setMine((m) => !m)}>
          {mine ? 'Show everyone’s' : 'Show only mine'}
        </button>
      </header>

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
                <span className="feed__count">{card.commentCount} comments</span>
              </p>
              {card.reviewPrompt && <p className="feed__prompt">“{card.reviewPrompt}”</p>}
              {card.excerpt && <p className="feed__excerpt">{card.excerpt}</p>}
            </Link>
          </li>
        ))}
      </ul>
    </main>
  )
}
