import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { BibleKind, EntryView, Scene } from '../api/types'

interface Props {
  workId: string
  /** The scene open in the editor, so entries can be linked to it in one click. */
  scene: Scene | null
}

// Singulars spelled out rather than derived: "people" does not lose an "s".
const KINDS: { value: BibleKind; label: string; one: string }[] = [
  { value: 'character', label: 'People', one: 'person' },
  { value: 'place', label: 'Places', one: 'place' },
  { value: 'note', label: 'Notes', one: 'note' },
]

/**
 * Characters, places and continuity notes, and which scenes they appear in.
 *
 * The link is the point: knowing a character's eye colour is easy, and finding
 * every scene you mentioned it in is not.
 */
export function StoryBible({ workId, scene }: Props) {
  const [entries, setEntries] = useState<EntryView[]>([])
  const [kind, setKind] = useState<BibleKind>('character')
  const [adding, setAdding] = useState(false)
  const [expanded, setExpanded] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.bible.list(workId).then(setEntries).catch(() => setEntries([]))
  }, [workId])

  useEffect(load, [load])

  const shown = entries.filter((entry) => entry.kind === kind)

  const toggleLink = async (entry: EntryView) => {
    if (!scene) return
    try {
      const updated = entry.sceneIds.includes(scene.id)
        ? await api.bible.unlink(entry.id, scene.id)
        : await api.bible.link(entry.id, scene.id)
      setEntries((current) => current.map((e) => (e.id === updated.id ? updated : e)))
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not change that link')
    }
  }

  return (
    <div className="bible">
      <div className="bible__kinds">
        {KINDS.map((option) => (
          <button
            key={option.value}
            type="button"
            className={kind === option.value ? 'is-active' : undefined}
            onClick={() => setKind(option.value)}
          >
            {option.label}
          </button>
        ))}
      </div>

      {error && <p className="error">{error}</p>}

      {shown.length === 0 && !adding && (
        <p className="empty">Nothing here yet. Add the ones you keep having to look up.</p>
      )}

      <ul className="bible__list">
        {shown.map((entry) => {
          const linked = scene ? entry.sceneIds.includes(scene.id) : false
          return (
            <li key={entry.id} className="bible__entry">
              <button
                type="button"
                className="bible__row"
                onClick={() => setExpanded(expanded === entry.id ? null : entry.id)}
              >
                <span className="bible__name">{entry.name}</span>
                <span className="bible__count">{entry.sceneIds.length}</span>
              </button>
              {entry.summary && <p className="bible__summary">{entry.summary}</p>}

              {expanded === entry.id && (
                <div className="bible__detail">
                  {entry.body && <p>{entry.body}</p>}
                  <p className="field__hint">
                    Appears in {entry.sceneIds.length} scene{entry.sceneIds.length === 1 ? '' : 's'}.
                  </p>
                  <div className="bible__actions">
                    {scene && (
                      <button type="button" onClick={() => toggleLink(entry)}>
                        {linked ? 'Remove from this scene' : 'Add to this scene'}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={async () => {
                        await api.bible.remove(entry.id)
                        load()
                      }}
                    >
                      Delete
                    </button>
                  </div>
                </div>
              )}
            </li>
          )
        })}
      </ul>

      {adding ? (
        <NewEntry
          workId={workId}
          kind={kind}
          onDone={() => { setAdding(false); load() }}
          onCancel={() => setAdding(false)}
        />
      ) : (
        <button type="button" className="linklike" onClick={() => setAdding(true)}>
          + Add {KINDS.find((k) => k.value === kind)?.one}
        </button>
      )}
    </div>
  )
}

function NewEntry({
  workId, kind, onDone, onCancel,
}: {
  workId: string
  kind: BibleKind
  onDone: () => void
  onCancel: () => void
}) {
  const [name, setName] = useState('')
  const [summary, setSummary] = useState('')
  const [busy, setBusy] = useState(false)

  return (
    <form
      className="bible__new"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        try {
          await api.bible.create(workId, { kind, name, summary })
          onDone()
        } finally {
          setBusy(false)
        }
      }}
    >
      <label>
        Name
        <input autoFocus value={name} onChange={(e) => setName(e.target.value)} required />
      </label>
      <label>
        One line
        <input
          value={summary}
          placeholder="What you keep having to look up"
          onChange={(e) => setSummary(e.target.value)}
        />
      </label>
      <div className="bible__actions">
        <button type="submit" disabled={busy || !name.trim()}>Add</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}
