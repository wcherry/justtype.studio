import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { BibleKind, EntryView, Scene, SketchField, SketchTemplate } from '../api/types'

interface Props {
  workId: string
  /** The scene open in the editor, so entries can be linked to it in one click. */
  scene: Scene | null
}

// Singulars spelled out rather than derived: "people" does not lose an "s".
const KINDS: { value: BibleKind; label: string; one: string }[] = [
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
 * colour mid-sentence does not have to go looking.
 */
export function StoryBible({ workId, scene }: Props) {
  const [entries, setEntries] = useState<EntryView[]>([])
  const [templates, setTemplates] = useState<SketchTemplate[]>([])
  const [kind, setKind] = useState<BibleKind>('character')
  const [adding, setAdding] = useState(false)
  const [expanded, setExpanded] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    api.bible.list(workId).then(setEntries).catch(() => setEntries([]))
  }, [workId])

  useEffect(load, [load])
  useEffect(() => {
    api.bible.templates().then(setTemplates).catch(() => setTemplates([]))
  }, [])

  const here = scene ? entries.filter((entry) => entry.sceneIds.includes(scene.id)) : []
  const shown = entries.filter((entry) => entry.kind === kind)

  const replace = (updated: EntryView) =>
    setEntries((current) => current.map((e) => (e.id === updated.id ? updated : e)))

  const toggleLink = async (entry: EntryView) => {
    if (!scene) return
    try {
      replace(
        entry.sceneIds.includes(scene.id)
          ? await api.bible.unlink(entry.id, scene.id)
          : await api.bible.link(entry.id, scene.id),
      )
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not change that link')
    }
  }

  // Keyed by list as well as entry: someone in this scene is in both lists,
  // and opening them in one should not open them in the other.
  const row = (list: string) => (entry: EntryView) => (
    <li key={entry.id} className="bible__entry">
      <button
        type="button"
        className="bible__row"
        onClick={() => setExpanded(expanded === `${list}:${entry.id}` ? null : `${list}:${entry.id}`)}
      >
        <span className="bible__name">{entry.name}</span>
        <span className="bible__count">{entry.sceneIds.length}</span>
      </button>
      {entry.summary && <p className="bible__summary">{entry.summary}</p>}
      {expanded === `${list}:${entry.id}` && (
        <EntryDetail
          entry={entry}
          scene={scene}
          linked={scene ? entry.sceneIds.includes(scene.id) : false}
          onToggleLink={() => toggleLink(entry)}
          onSaved={replace}
          onDeleted={load}
          onError={setError}
        />
      )}
    </li>
  )

  return (
    <div className="bible">
      {here.length > 0 && (
        <section className="bible__here">
          <h4>In this scene</h4>
          <ul className="bible__list">{here.map(row('here'))}</ul>
        </section>
      )}

      <div className="bible__kinds">
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

      <ul className="bible__list">{shown.map(row('all'))}</ul>

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
  entry, scene, linked, onToggleLink, onSaved, onDeleted, onError,
}: {
  entry: EntryView
  scene: Scene | null
  linked: boolean
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
      onSaved(await api.bible.update(entry.id, { fields, body, summary }))
      setEditing(false)
    } catch (e) {
      onError(e instanceof Error ? e.message : 'Could not save that')
    } finally {
      setBusy(false)
    }
  }

  const filled = entry.fields.filter((f) => (f.value ?? '').trim())

  return (
    <div className="bible__detail">
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
          <div className="bible__actions">
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
        <div className="bible__actions">
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
              await api.bible.remove(entry.id)
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
  kind: BibleKind
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
      className="bible__new"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        try {
          onDone(await api.bible.create(workId, {
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
      <div className="bible__actions">
        <button type="submit" disabled={busy || !name.trim()}>Add</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}
