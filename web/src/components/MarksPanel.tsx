import { useState } from 'react'
import type { Annotation } from '../api/types'

type Filter = 'all' | 'bookmark' | 'highlight'

/**
 * Your highlights and bookmarks, in reading order, each one a way back to its
 * passage. Only you see them.
 */
export function MarksPanel({
  marks, scenes, lost = new Set(), onOpen, onRemove, onRename,
}: {
  marks: Annotation[]
  /** Every scene in reading order, for grouping and titles. */
  scenes: { id: string; title: string }[]
  /** Marks whose words can no longer be found in the draft. */
  lost?: Set<string>
  onOpen: (id: string) => void
  onRemove: (id: string) => void
  onRename: (id: string, note: string) => Promise<unknown>
}) {
  const [filter, setFilter] = useState<Filter>('all')
  const [editing, setEditing] = useState<string | null>(null)
  const [name, setName] = useState('')

  const shown = marks.filter((m) => filter === 'all' || m.kind === filter)
  const order = new Map(scenes.map((s, i) => [s.id, i]))
  const groups = scenes
    .map((scene) => ({
      scene,
      marks: shown
        .filter((m) => m.sceneId === scene.id)
        .sort((a, b) => a.fromPos - b.fromPos),
    }))
    .filter((g) => g.marks.length > 0)
  // Marks on scenes since deleted have nowhere to go back to.
  const orphaned = shown.filter((m) => !order.has(m.sceneId))

  return (
    <div className="marks">
      <p className="field__hint">
        Select text and right-click it (or ⌘-click) to highlight or bookmark it. Only you see these.
      </p>
      <div className="segmented" role="group" aria-label="Show">
        {(['all', 'bookmark', 'highlight'] as Filter[]).map((f) => (
          <button
            key={f}
            type="button"
            className={filter === f ? 'is-active' : undefined}
            aria-pressed={filter === f}
            onClick={() => setFilter(f)}
          >
            {f === 'all' ? 'All' : f === 'bookmark' ? 'Bookmarks' : 'Highlights'}
          </button>
        ))}
      </div>

      {shown.length === 0 && <p className="empty">Nothing marked yet.</p>}

      {groups.map(({ scene, marks: list }) => (
        <section key={scene.id} className="marks__scene">
          <h4>{scene.title}</h4>
          <ul>
            {list.map((mark) => (
              <li key={mark.id} className={`marks__item marks__item--${mark.kind}`}>
                {editing === mark.id ? (
                  <form
                    className="marks__rename"
                    onSubmit={async (event) => {
                      event.preventDefault()
                      await onRename(mark.id, name)
                      setEditing(null)
                    }}
                  >
                    <input
                      autoFocus
                      value={name}
                      maxLength={500}
                      placeholder={mark.kind === 'bookmark' ? 'Name this bookmark' : 'A line about it'}
                      onChange={(e) => setName(e.target.value)}
                    />
                    <button type="submit">Save</button>
                    <button type="button" className="linklike" onClick={() => setEditing(null)}>Cancel</button>
                  </form>
                ) : (
                  <button
                    type="button"
                    className="marks__open"
                    disabled={lost.has(mark.id)}
                    onClick={() => onOpen(mark.id)}
                  >
                    {mark.note && <span className="marks__note">{mark.note}</span>}
                    <span className="marks__quote">{clip(mark.quote)}</span>
                  </button>
                )}
                {lost.has(mark.id) && (
                  <p className="field__hint">These words are no longer in the draft.</p>
                )}
                <div className="marks__actions">
                  <span className="field__hint">{mark.kind === 'bookmark' ? 'Bookmark' : 'Highlight'}</span>
                  <button
                    type="button"
                    className="linklike"
                    onClick={() => {
                      setEditing(mark.id)
                      setName(mark.note ?? '')
                    }}
                  >
                    {mark.kind === 'bookmark' ? 'Name' : 'Note'}
                  </button>
                  <button type="button" className="linklike" onClick={() => onRemove(mark.id)}>
                    Remove
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      ))}

      {orphaned.length > 0 && (
        <section className="marks__scene">
          <h4>Scenes no longer in the book</h4>
          <ul>
            {orphaned.map((mark) => (
              <li key={mark.id} className="marks__item">
                <span className="marks__quote">{clip(mark.quote)}</span>
                <div className="marks__actions">
                  <button type="button" className="linklike" onClick={() => onRemove(mark.id)}>Remove</button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}

function clip(quote: string) {
  return quote.length > 220 ? `${quote.slice(0, 220).trimEnd()}…` : quote
}
