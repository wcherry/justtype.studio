import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { api } from '../api/client'
import type { Genre, Template, Work, WorkKind } from '../api/types'
import { GENRES } from '../components/labels'

const KINDS: { value: WorkKind; label: string }[] = [
  { value: 'novel', label: 'Novel' },
  { value: 'novella', label: 'Novella' },
  { value: 'short_story', label: 'Short story' },
  { value: 'serial', label: 'Serial' },
]

export function LibraryPage() {
  const [works, setWorks] = useState<Work[]>([])
  const [templates, setTemplates] = useState<Template[]>([])
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const navigate = useNavigate()

  const load = () => {
    api.works.list().then(setWorks).catch((e) => setError(e.message))
  }

  useEffect(() => {
    load()
    api.templates.list().then(setTemplates).catch(() => undefined)
  }, [])

  return (
    <main className="library">
      <header className="library__header">
        <h1>Your desk</h1>
        <button type="button" onClick={() => setCreating((c) => !c)}>
          {creating ? 'Cancel' : 'Start something new'}
        </button>
      </header>

      {error && <p className="error">{error}</p>}

      {creating && (
        <NewWorkForm
          templates={templates}
          onCreated={(workId) => navigate(`/works/${workId}`)}
        />
      )}

      {works.length === 0 && !creating && (
        <p className="empty">Nothing here yet. Start something — a template will give you a spine to hang it on.</p>
      )}

      <ul className="library__list">
        {works.map((work) => (
          <li key={work.id}>
            <Link to={`/works/${work.id}`} className="library__card">
              <h2>{work.title}</h2>
              <p className="library__meta">
                <span className={`badge badge--${work.status}`}>{work.status}</span>
                {KINDS.find((k) => k.value === work.kind)?.label} · {work.wordCount.toLocaleString()} words
              </p>
              {work.synopsis && <p className="library__synopsis">{work.synopsis}</p>}
              <p className="library__meta">Edited {new Date(work.updatedAt).toLocaleDateString()}</p>
            </Link>
          </li>
        ))}
      </ul>
    </main>
  )
}

function NewWorkForm({
  templates,
  onCreated,
}: {
  templates: Template[]
  onCreated: (workId: string) => void
}) {
  const [title, setTitle] = useState('')
  const [kind, setKind] = useState<WorkKind>('novel')
  const [templateId, setTemplateId] = useState<string>('tpl-blank')
  const [genre, setGenre] = useState<Genre | ''>('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const selected = templates.find((t) => t.id === templateId)

  return (
    <form
      className="new-work"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        setError(null)
        try {
          const outline = await api.works.create({ title, kind, templateId, genre: genre || undefined })
          onCreated(outline.id)
        } catch (e) {
          setError(e instanceof Error ? e.message : 'Could not create that work')
        } finally {
          setBusy(false)
        }
      }}
    >
      <label>
        Title
        <input value={title} onChange={(e) => setTitle(e.target.value)} required autoFocus />
      </label>
      <label>
        Form
        <select value={kind} onChange={(e) => setKind(e.target.value as WorkKind)}>
          {KINDS.map((k) => (
            <option key={k.value} value={k.value}>{k.label}</option>
          ))}
        </select>
      </label>
      <label>
        Genre
        <select value={genre} onChange={(e) => setGenre(e.target.value as Genre | '')}>
          <option value="">Not sure yet</option>
          {GENRES.map((g) => (
            <option key={g.value} value={g.value}>{g.label}</option>
          ))}
        </select>
      </label>
      <label>
        Template
        <select value={templateId} onChange={(e) => setTemplateId(e.target.value)}>
          {templates.map((t) => (
            <option key={t.id} value={t.id}>{t.name}</option>
          ))}
        </select>
      </label>
      {selected && <p className="new-work__hint">{selected.description}</p>}
      {error && <p className="error">{error}</p>}
      <button type="submit" disabled={busy || !title.trim()}>Create</button>
    </form>
  )
}
