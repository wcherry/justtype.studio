import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { NotebookKind, EntryView, Scene, SketchField, SketchTemplate } from '../api/types'

interface Props {
  workId: string
  /** The scene open in the editor, so entries can be linked to it in one click. */
  scene: Scene | null
}

// Singulars spelled out rather than derived: "people" does not lose an "s".
const KINDS: { value: NotebookKind; label: string; one: string }[] = [
  { value: 'character', label: 'People', one: 'person' },
  { value: 'place', label: 'Places', one: 'place' },
  { value: 'thing', label: 'Things', one: 'thing' },
  { value: 'group', label: 'Groups', one: 'group' },
  { value: 'note', label: 'Notes', one: 'note' },
]

/**
 * Characters, places, things, groups and continuity notes; their sketches; and
 * which scenes they appear in.
 *
 * Whoever is in the scene open now comes first, so a writer checking her eye
 * colour mid-sentence does not have to go looking. Each kind is in the
 * writer's own order. An entry's row is the whole of its handling: click to
 * open it, press and drag to move it, double-click to edit it; from the
 * keyboard, Enter opens it and Alt+↑/↓ moves it.
 */
export function Notebook({ workId, scene }: Props) {
  const [entries, setEntries] = useState<EntryView[]>([])
  const [templates, setTemplates] = useState<SketchTemplate[]>([])
  const [kind, setKind] = useState<NotebookKind>('character')
  const [adding, setAdding] = useState(false)
  const [expanded, setExpanded] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** The entry being dragged, and where it would land. */
  const [dragging, setDragging] = useState<string | null>(null)
  const [dropAt, setDropAt] = useState<{ id: string; after: boolean } | null>(null)
  /** Said aloud after a move from the keyboard. */
  const [announce, setAnnounce] = useState('')
  /** An entry double-clicked: it opens straight into editing. */
  const [editRequest, setEditRequest] = useState<string | null>(null)

  const load = useCallback(() => {
    api.notebook.list(workId).then(setEntries).catch(() => setEntries([]))
  }, [workId])

  useEffect(load, [load])
  useEffect(() => {
    api.notebook.templates().then(setTemplates).catch(() => setTemplates([]))
  }, [])

  const here = scene ? entries.filter((entry) => entry.sceneIds.includes(scene.id)) : []
  const shown = entries.filter((entry) => entry.kind === kind)

  const replace = (updated: EntryView) =>
    setEntries((current) => current.map((e) => (e.id === updated.id ? updated : e)))

  /** Shows the new order at once and keeps it; puts it back if it cannot be kept. */
  const reorder = (ids: string[]) => {
    const rank = new Map(ids.map((id, i) => [id, i]))
    setEntries((current) => {
      const others = current.filter((e) => e.kind !== kind)
      const ofKind = current.filter((e) => e.kind === kind)
      ofKind.sort((a, b) => (rank.get(a.id) ?? 0) - (rank.get(b.id) ?? 0))
      return [...others, ...ofKind]
    })
    api.notebook.reorder(workId, kind, ids).catch((e) => {
      setError(e instanceof Error ? e.message : 'Could not keep that order')
      load()
    })
  }

  /** `id` moved to just before or after `target`. */
  const dropped = (id: string, target: string, after: boolean) => {
    if (id === target) return
    const ids = shown.map((e) => e.id).filter((x) => x !== id)
    const at = ids.indexOf(target)
    if (at < 0) return
    ids.splice(after ? at + 1 : at, 0, id)
    reorder(ids)
  }

  const nudge = (entry: EntryView, delta: number) => {
    const ids = shown.map((e) => e.id)
    const from = ids.indexOf(entry.id)
    const to = from + delta
    if (from < 0 || to < 0 || to >= ids.length) return
    ids.splice(from, 1)
    ids.splice(to, 0, entry.id)
    reorder(ids)
    setAnnounce(`${entry.name}, ${to + 1} of ${ids.length}`)
  }

  const toggleLink = async (entry: EntryView) => {
    if (!scene) return
    try {
      replace(
        entry.sceneIds.includes(scene.id)
          ? await api.notebook.unlink(entry.id, scene.id)
          : await api.notebook.link(entry.id, scene.id),
      )
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not change that link')
    }
  }

  // Keyed by list as well as entry: someone in this scene is in both lists,
  // and opening them in one should not open them in the other.
  // Only the full list of a kind is arranged; "In this scene" follows it.
  const row = (list: string) => (entry: EntryView) => {
    const arranging = list === 'all' && shown.length > 1
    const classes = ['notebook__entry']
    if (dragging === entry.id) classes.push('is-dragging')
    if (dropAt?.id === entry.id && dragging && dragging !== entry.id) {
      classes.push(dropAt.after ? 'is-drop-after' : 'is-drop-before')
    }
    return (
    <li
      key={entry.id}
      className={classes.join(' ')}
      onDragOver={arranging ? (e) => {
        if (!dragging) return
        e.preventDefault()
        e.dataTransfer.dropEffect = 'move'
        const box = e.currentTarget.getBoundingClientRect()
        const after = e.clientY > box.top + box.height / 2
        if (dropAt?.id !== entry.id || dropAt.after !== after) setDropAt({ id: entry.id, after })
      } : undefined}
      onDrop={arranging ? (e) => {
        e.preventDefault()
        if (dragging && dropAt) dropped(dragging, dropAt.id, dropAt.after)
        setDragging(null)
        setDropAt(null)
      } : undefined}
    >
      {/*
        A row rather than a button: a button cannot be dragged in every
        browser. A click that does not move opens it; one that moves drags it.
      */}
      <div
        role="button"
        tabIndex={0}
        aria-expanded={expanded === `${list}:${entry.id}`}
        aria-keyshortcuts={arranging ? 'Alt+ArrowUp Alt+ArrowDown' : undefined}
        className={arranging ? 'notebook__row is-arrangeable' : 'notebook__row'}
        title={arranging ? 'Click to open · drag to move · double-click to edit' : 'Click to open · double-click to edit'}
        draggable={arranging}
        onClick={(e) => {
          // The second click of a double-click edits: the first has opened
          // it, so it stays open rather than closing again.
          if (e.detail >= 2) {
            setExpanded(`${list}:${entry.id}`)
            setEditRequest(`${list}:${entry.id}`)
          } else {
            setExpanded(expanded === `${list}:${entry.id}` ? null : `${list}:${entry.id}`)
          }
        }}
        onKeyDown={(e) => {
          if (e.target !== e.currentTarget) return
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            setExpanded(expanded === `${list}:${entry.id}` ? null : `${list}:${entry.id}`)
          } else if (arranging && e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
            e.preventDefault()
            nudge(entry, e.key === 'ArrowUp' ? -1 : 1)
          }
        }}
        onDragStart={arranging ? (e) => {
          e.dataTransfer.effectAllowed = 'move'
          e.dataTransfer.setData('text/plain', entry.id)
          const item = e.currentTarget.closest('li')
          if (item) e.dataTransfer.setDragImage(item, 16, 12)
          setDragging(entry.id)
        } : undefined}
        onDragEnd={arranging ? () => {
          setDragging(null)
          setDropAt(null)
        } : undefined}
      >
        <span className="notebook__name">{entry.name}</span>
        <span className="notebook__count">{entry.sceneIds.length}</span>
      </div>
      {entry.summary && <p className="notebook__summary">{entry.summary}</p>}
      {expanded === `${list}:${entry.id}` && (
        <EntryDetail
          entry={entry}
          scene={scene}
          linked={scene ? entry.sceneIds.includes(scene.id) : false}
          startEditing={editRequest === `${list}:${entry.id}`}
          onEditing={() => setEditRequest(null)}
          onToggleLink={() => toggleLink(entry)}
          onSaved={replace}
          onDeleted={load}
          onError={setError}
        />
      )}
    </li>
    )
  }

  return (
    <div className="notebook">
      {here.length > 0 && (
        <section className="notebook__here">
          <h4>In this scene</h4>
          <ul className="notebook__list">{here.map(row('here'))}</ul>
        </section>
      )}

      <div className="notebook__kinds">
        {KINDS.map((option) => (
          <button
            key={option.value}
            type="button"
            className={kind === option.value ? 'is-active' : undefined}
            onClick={() => { setKind(option.value); setAdding(false) }}
          >
            {option.label}
          </button>
        ))}
      </div>

      {error && <p className="error">{error}</p>}

      {shown.length === 0 && !adding && (
        <p className="empty">Nothing here yet. Add the ones you keep having to look up.</p>
      )}

      <ul className="notebook__list">{shown.map(row('all'))}</ul>
      <p className="visually-hidden" aria-live="polite">{announce}</p>

      {adding ? (
        <NewEntry
          workId={workId}
          kind={kind}
          templates={templates.filter((t) => t.kind === kind)}
          onDone={(entry) => { setAdding(false); load(); setExpanded(`all:${entry.id}`) }}
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

/** One entry opened: its sketch, its notes, and its place in the scene. */
function EntryDetail({
  entry, scene, linked, startEditing, onEditing, onToggleLink, onSaved, onDeleted, onError,
}: {
  entry: EntryView
  scene: Scene | null
  linked: boolean
  /** Asked to open in editing, by a double-click on its row. */
  startEditing: boolean
  /** The request has been taken up. */
  onEditing: () => void
  onToggleLink: () => void
  onSaved: (entry: EntryView) => void
  onDeleted: () => void
  onError: (message: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [fields, setFields] = useState<SketchField[]>(entry.fields)
  const [body, setBody] = useState(entry.body ?? '')
  const [summary, setSummary] = useState(entry.summary ?? '')
  const [busy, setBusy] = useState(false)
  const [newField, setNewField] = useState('')

  useEffect(() => {
    if (!startEditing) return
    setEditing(true)
    onEditing()
  }, [startEditing, onEditing])

  const addField = () => {
    const label = newField.trim()
    if (!label) return
    // A key of its own from the start, so the list stays keyed while it is
    // being edited; the server keeps whatever unique key it is given.
    const key = `own-${Date.now().toString(36)}`
    setFields((current) => [...current, { key, label, value: '' }])
    setNewField('')
  }

  const save = async () => {
    setBusy(true)
    try {
      onSaved(await api.notebook.update(entry.id, { fields, body, summary }))
      setEditing(false)
    } catch (e) {
      onError(e instanceof Error ? e.message : 'Could not save that')
    } finally {
      setBusy(false)
    }
  }

  const filled = entry.fields.filter((f) => (f.value ?? '').trim())

  return (
    <div className="notebook__detail">
      {editing ? (
        <div className="sketch sketch--editing">
          <label>
            One line
            <input value={summary} onChange={(e) => setSummary(e.target.value)} />
          </label>
          {fields.map((field, i) => (
            <label key={field.key}>
              {field.label}
              <textarea
                rows={2}
                value={field.value ?? ''}
                placeholder={field.prompt ?? ''}
                onChange={(e) =>
                  setFields((current) =>
                    current.map((f, j) => (j === i ? { ...f, value: e.target.value } : f)),
                  )
                }
              />
            </label>
          ))}
          <div className="sketch__add">
            <input
              value={newField}
              placeholder="A question of your own"
              onChange={(e) => setNewField(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  addField()
                }
              }}
            />
            <button type="button" disabled={!newField.trim()} onClick={addField}>Add field</button>
          </div>
          <label>
            Notes
            <textarea rows={4} value={body} onChange={(e) => setBody(e.target.value)} />
          </label>
          <div className="notebook__actions">
            <button type="button" disabled={busy} onClick={() => void save()}>Save</button>
            <button
              type="button"
              onClick={() => {
                setFields(entry.fields)
                setBody(entry.body ?? '')
                setSummary(entry.summary ?? '')
                setEditing(false)
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <>
          {filled.length > 0 && (
            <dl className="sketch">
              {filled.map((field) => (
                <div key={field.key} className="sketch__field">
                  <dt>{field.label}</dt>
                  <dd>{field.value}</dd>
                </div>
              ))}
            </dl>
          )}
          {entry.fields.length > filled.length && (
            <p className="field__hint">
              {entry.fields.length - filled.length} of {entry.fields.length} questions still open.
            </p>
          )}
          {entry.body && <p className="sketch__notes">{entry.body}</p>}
        </>
      )}

      <p className="field__hint">
        Appears in {entry.sceneIds.length} scene{entry.sceneIds.length === 1 ? '' : 's'}.
      </p>
      {!editing && (
        <div className="notebook__actions">
          <button type="button" onClick={() => setEditing(true)}>
            {entry.fields.length > 0 ? 'Fill in the sketch' : 'Edit'}
          </button>
          {scene && (
            <button type="button" onClick={onToggleLink}>
              {linked ? 'Remove from this scene' : 'Add to this scene'}
            </button>
          )}
          <button
            type="button"
            onClick={async () => {
              await api.notebook.remove(entry.id)
              onDeleted()
            }}
          >
            Delete
          </button>
        </div>
      )}
    </div>
  )
}

function NewEntry({
  workId, kind, templates, onDone, onCancel,
}: {
  workId: string
  kind: NotebookKind
  templates: SketchTemplate[]
  onDone: (entry: EntryView) => void
  onCancel: () => void
}) {
  const [name, setName] = useState('')
  const [summary, setSummary] = useState('')
  // Started from the first template for the kind, when there is one: a blank
  // page is the thing a sketch exists to avoid.
  const [templateId, setTemplateId] = useState<string>(templates[0]?.id ?? '')
  const [busy, setBusy] = useState(false)
  const chosen = templates.find((t) => t.id === templateId)

  return (
    <form
      className="notebook__new"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        try {
          onDone(await api.notebook.create(workId, {
            kind, name, summary, templateId: templateId || undefined,
          }))
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
      {templates.length > 0 && (
        <label>
          Start from
          <select value={templateId} onChange={(e) => setTemplateId(e.target.value)}>
            {templates.map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            <option value="">A blank page</option>
          </select>
          {chosen && (
            <span className="field__hint">
              {chosen.description} {chosen.fields.map((f) => f.label).join(' · ')}
            </span>
          )}
        </label>
      )}
      <div className="notebook__actions">
        <button type="submit" disabled={busy || !name.trim()}>Add</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}
