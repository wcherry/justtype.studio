import { useCallback, useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { inbox } from '../api/client'
import type { DigestView, InboxItemView } from '../api/types'
import { notifyInboxChanged } from '../state/inbox'

/**
 * What happened that you should hear about: critiques and replies on Daily
 * Prompts answers, so far. Yesterday's summary is pinned at the top until read.
 */
export function InboxPage() {
  const navigate = useNavigate()
  const [items, setItems] = useState<InboxItemView[]>([])
  const [digest, setDigest] = useState<DigestView | null>(null)
  const [cursor, setCursor] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async (from: string | null) => {
    try {
      const page = await inbox.list(from)
      if (!from) setDigest(page.digest ?? null)
      setItems((current) => (from ? [...current, ...page.items] : page.items))
      setCursor(page.nextCursor ?? null)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load the inbox')
    } finally {
      setLoaded(true)
    }
  }, [])

  useEffect(() => {
    void load(null)
  }, [load])

  const open = async (item: InboxItemView) => {
    if (!item.readAt) {
      await inbox.read(item.id).catch(() => undefined)
      notifyInboxChanged()
    }
    navigate(item.link)
  }

  return (
    <main className="feed inbox">
      <header className="feed__header">
        <h1>Inbox</h1>
        <button
          type="button"
          disabled={!items.some((i) => !i.readAt) && !digest}
          onClick={async () => {
            await inbox.readAll()
            notifyInboxChanged()
            setDigest(null)
            setItems((current) => current.map((i) => ({ ...i, readAt: i.readAt ?? new Date().toISOString() })))
          }}
        >
          Mark all read
        </button>
      </header>
      {error && <p className="error">{error}</p>}
      {digest && (
        <section className="inbox__digest">
          <p>{digest.text}</p>
          <p className="inbox__digest-actions">
            <Link to="/prompts">Daily Prompts</Link>
            <button
              type="button"
              className="linklike"
              onClick={async () => {
                await inbox.readDigest(digest.day)
                notifyInboxChanged()
                setDigest(null)
              }}
            >
              Dismiss
            </button>
          </p>
        </section>
      )}
      {loaded && items.length === 0 && !digest && <p className="empty">Nothing yet. Critiques and replies to you will show up here.</p>}
      <ul className="inbox__list">
        {items.map((item) => (
          <li key={item.id} className={item.readAt ? 'inbox__item' : 'inbox__item is-unread'}>
            <button type="button" className="inbox__open" onClick={() => void open(item)}>
              <span className="inbox__text">{item.text}</span>
              <time className="field__hint" dateTime={item.createdAt}>{new Date(item.createdAt).toLocaleString()}</time>
            </button>
          </li>
        ))}
      </ul>
      {cursor && <button type="button" onClick={() => void load(cursor)}>Older</button>}
    </main>
  )
}
