import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { prompts } from '../api/client'
import type { EntryCard } from '../api/types'

type Sort = 'newest' | 'fewest_critiques'

/**
 * Everyone's submitted answers to one prompt, newest first or fewest
 * critiques first, so a writer who has just arrived is not buried under the
 * ones everyone already read. Late answers are in it too, marked.
 */
export function EntryFeed({ day, heading }: { day: string; heading: string }) {
  const [sort, setSort] = useState<Sort>('newest')
  const [cards, setCards] = useState<EntryCard[]>([])
  const [cursor, setCursor] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(false)

  const load = useCallback(
    async (from: string | null) => {
      try {
        const page = await prompts.feed(day, sort, from)
        setCards((current) => (from ? [...current, ...page.entries] : page.entries))
        setCursor(page.nextCursor ?? null)
        setError(null)
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Could not load the answers')
      } finally {
        setLoaded(true)
      }
    },
    [day, sort],
  )

  useEffect(() => {
    void load(null)
  }, [load])

  return (
    <section className="prompts__section">
      <header className="prompts__section-head">
        <h2>{heading}</h2>
        <div className="segmented" role="group" aria-label="Order">
          <button type="button" className={sort === 'newest' ? 'is-active' : undefined} onClick={() => setSort('newest')}>
            Newest
          </button>
          <button
            type="button"
            className={sort === 'fewest_critiques' ? 'is-active' : undefined}
            onClick={() => setSort('fewest_critiques')}
          >
            Fewest critiques
          </button>
        </div>
      </header>
      {error && <p className="error">{error}</p>}
      {loaded && !error && cards.length === 0 && <p className="empty">No one has submitted an answer yet.</p>}
      <ul className="feed__list">
        {cards.map((card) => (
          <li key={card.id} className="feed__card entry-card">
            <p className="feed__meta">
              <Link to={`/people/${card.author.id}`}>{card.author.displayName}</Link>
              {card.mine && <span className="badge">yours</span>}
              {' · '}
              {card.wordCount.toLocaleString()} words
              {' · '}
              {card.critiqueCount === 1 ? '1 critique' : `${card.critiqueCount} critiques`}
              {card.late && <span className="badge badge--late" title="Submitted after the prompt's day ended">late</span>}
              {card.editedAt && <span className="badge" title={`Edited ${new Date(card.editedAt).toLocaleString()}`}>edited</span>}
            </p>
            <Link to={`/prompts/${card.day}/entries/${card.id}`} className="entry-card__excerpt">
              {card.excerpt}
            </Link>
          </li>
        ))}
      </ul>
      {cursor && (
        <button type="button" onClick={() => void load(cursor)}>
          More
        </button>
      )}
    </section>
  )
}
