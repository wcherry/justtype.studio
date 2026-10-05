import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import { BlockList } from '../components/BlockList'
import { CircleManager } from '../components/CircleManager'
import { FeedbackPanel } from '../components/FeedbackPanel'
import { SincePanel } from '../components/SincePanel'
import { PublicationTile } from '../components/PublicationTile'
import { ReviewPanel } from '../components/ReviewPanel'
import type { PublicationCard } from '../api/types'

type Scope = 'everyone' | 'following' | 'mine'

const HEADINGS: Record<Scope, string> = {
  everyone: 'Looking for readers',
  following: 'From people you follow',
  mine: 'Your publications',
}

export function FeedPage() {
  const [cards, setCards] = useState<PublicationCard[]>([])
  const [scope, setScope] = useState<Scope>('everyone')
  const mine = scope === 'mine'
  const [managing, setManaging] = useState<string | null>(null)
  const [reading, setReading] = useState<string | null>(null)
  const [comparing, setComparing] = useState<string | null>(null)
  const [showCircles, setShowCircles] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.publications
      .feed({ mine, following: scope === 'following' })
      .then(setCards)
      .catch((e) => setError(e.message))
  }, [mine, scope])

  useEffect(() => {
    load()
  }, [load])

  return (
    <main className="feed">
      <header className="feed__header">
        <h1>{HEADINGS[scope]}</h1>
        <div className="feed__actions">
          <div className="segmented" role="group" aria-label="Whose work">
            {(['everyone', 'following', 'mine'] as const).map((value) => (
              <button
                key={value}
                type="button"
                className={scope === value ? 'is-active' : undefined}
                onClick={() => setScope(value)}
              >
                {value === 'everyone' ? 'Everyone' : value === 'following' ? 'Following' : 'Mine'}
              </button>
            ))}
          </div>
          <button type="button" onClick={() => setShowCircles((c) => !c)}>
            {showCircles ? 'Hide circles' : 'Reader circles'}
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

          <h2>Blocked readers</h2>
          <BlockList />
        </section>
      )}

      {error && <p className="error">{error}</p>}
      {cards.length === 0 && (
        <p className="empty">
          {scope === 'following'
            ? 'Nobody you follow has anything open right now. Follow an author from their name on any card.'
            : 'Nothing on offer right now.'}
        </p>
      )}

      <ul className="feed__list">
        {cards.map((card) => (
          <li key={card.id}>
            <PublicationTile card={card} />
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
                  <button
                    type="button"
                    className="linklike"
                    onClick={() => setComparing(comparing === card.id ? null : card.id)}
                  >
                    {comparing === card.id ? 'Hide changes' : 'Since they read it'}
                  </button>
                </div>
                {managing === card.id && <ReviewPanel publication={card} onChanged={load} />}
                {reading === card.id && <FeedbackPanel publication={card} />}
                {comparing === card.id && <SincePanel publication={card} />}
              </>
            )}
          </li>
        ))}
      </ul>
    </main>
  )
}
