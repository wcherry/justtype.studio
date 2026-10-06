import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { FeedFilters } from '../api/client'
import { BlockList } from '../components/BlockList'
import { CircleManager } from '../components/CircleManager'
import { FeedbackPanel } from '../components/FeedbackPanel'
import { SincePanel } from '../components/SincePanel'
import { PublicationTile } from '../components/PublicationTile'
import { ReviewPanel } from '../components/ReviewPanel'
import { GENRES, LENGTHS, SORTS, WINDOWS } from '../components/labels'
import type { PublicationCard, WorkKind } from '../api/types'

const FORMS: { value: WorkKind; label: string }[] = [
  { value: 'novel', label: 'Novel' },
  { value: 'novella', label: 'Novella' },
  { value: 'short_story', label: 'Short story' },
  { value: 'serial', label: 'Serial' },
]

type Scope = 'everyone' | 'following' | 'mine'

const HEADINGS: Record<Scope, string> = {
  everyone: 'Looking for readers',
  following: 'From people you follow',
  mine: 'Your publications',
}

export function FeedPage() {
  const [cards, setCards] = useState<PublicationCard[]>([])
  const [scope, setScope] = useState<Scope>('everyone')
  const [filters, setFilters] = useState<FeedFilters>({})
  const [q, setQ] = useState('')
  const [searching, setSearching] = useState('')
  // Searched a moment after typing stops, not on every key.
  useEffect(() => {
    const timer = window.setTimeout(() => setSearching(q), 300)
    return () => window.clearTimeout(timer)
  }, [q])
  const mine = scope === 'mine'
  const queue = filters.sort === 'needs_readers' && !mine
  const [managing, setManaging] = useState<string | null>(null)
  const [reading, setReading] = useState<string | null>(null)
  const [comparing, setComparing] = useState<string | null>(null)
  const [showCircles, setShowCircles] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.publications
      .feed({ ...filters, mine, following: scope === 'following', q: searching })
      .then((next) => {
        setCards(next)
        setError(null)
      })
      .catch((e) => setError(e.message))
  }, [mine, scope, filters, searching])

  const narrow = <K extends keyof FeedFilters>(key: K, value: string) =>
    setFilters((current) => ({ ...current, [key]: (value || undefined) as FeedFilters[K] }))

  useEffect(() => {
    load()
  }, [load])

  return (
    <main className="feed">
      <header className="feed__header">
        <h1>{queue ? 'Waiting for a first reader' : HEADINGS[scope]}</h1>
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

      <div className="feed__filters">
        <input
          type="search"
          className="feed__search"
          placeholder="Find a title, an author, a line…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <select aria-label="Form" value={filters.kind ?? ''} onChange={(e) => narrow('kind', e.target.value)}>
          <option value="">Any form</option>
          {FORMS.map((f) => <option key={f.value} value={f.value}>{f.label}</option>)}
        </select>
        <select aria-label="Genre" value={filters.genre ?? ''} onChange={(e) => narrow('genre', e.target.value)}>
          <option value="">Any genre</option>
          {GENRES.map((g) => <option key={g.value} value={g.value}>{g.label}</option>)}
        </select>
        <select aria-label="Length" value={filters.length ?? ''} onChange={(e) => narrow('length', e.target.value)}>
          <option value="">Any length</option>
          {LENGTHS.map((l) => <option key={l.value} value={l.value}>{l.label}</option>)}
        </select>
        <select aria-label="Review window" value={filters.window ?? ''} onChange={(e) => narrow('window', e.target.value)}>
          <option value="">Any window</option>
          {WINDOWS.map((w) => <option key={w.value} value={w.value}>{w.label}</option>)}
        </select>
        {!mine && (
          <div className="segmented" role="group" aria-label="Order">
            {SORTS.map((sort) => (
              <button
                key={sort.value}
                type="button"
                className={(filters.sort ?? 'newest') === sort.value ? 'is-active' : undefined}
                onClick={() => narrow('sort', sort.value === 'newest' ? '' : sort.value)}
              >
                {sort.label}
              </button>
            ))}
          </div>
        )}
      </div>
      {queue && (
        <p className="field__hint">
          Least-read first, leaving out your own and anything you have already opened. Reading for
          other writers moves your own work up this list for everyone else.
        </p>
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
