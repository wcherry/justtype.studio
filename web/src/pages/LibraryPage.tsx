import { useEffect, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { api, downloadDraftExport, printDraft } from '../api/client'
import type { Genre, Template, Work, WorkKind } from '../api/types'
import { GENRES } from '../components/labels'
import { ConfirmDialog, RenameDialog } from '../components/Dialogs'
import { ExportDialog } from '../components/ExportDialog'
import { ImportDialog } from '../components/ImportDialog'
import { ContextMenu, Menu, type MenuItem } from '../components/Menu'
import { useAuth } from '../state/auth'

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
  const [importing, setImporting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  /** What a book's menu has open: a dialog about it, or a menu at the pointer. */
  const [dialog, setDialog] = useState<{ kind: 'rename' | 'delete' | 'export'; work: Work } | null>(null)
  const [pointerMenu, setPointerMenu] = useState<{ x: number; y: number; work: Work } | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const navigate = useNavigate()
  const { user } = useAuth()

  useEffect(() => {
    if (!notice) return
    const timer = setTimeout(() => setNotice(null), 4000)
    return () => clearTimeout(timer)
  }, [notice])

  /**
   * What can be done with a whole book from the desk. Exporting, printing and
   * copying are the owner's; deleting is only for the person who started it.
   */
  const bookMenu = (work: Work): MenuItem[] => {
    const mine = !!user && work.ownerId === user.id
    const tryTo = (what: string, run: () => Promise<unknown>) => () => {
      run().catch((e) => setNotice(e instanceof Error ? e.message : `Could not ${what}`))
    }
    return [
      { key: 'open', label: 'Open', onSelect: () => navigate(`/works/${work.id}`) },
      { key: 'rename', label: 'Rename…', disabled: !mine, onSelect: () => setDialog({ kind: 'rename', work }) },
      {
        key: 'duplicate',
        label: 'Duplicate',
        disabled: !mine,
        onSelect: tryTo('duplicate it', async () => {
          const copy = await api.works.duplicate(work.id)
          load()
          setNotice(`Made “${copy.title}”: the whole book, as a new draft.`)
        }),
      },
      { key: 'export', label: 'Export…', divider: true, disabled: !mine, onSelect: () => setDialog({ kind: 'export', work }) },
      {
        key: 'print',
        label: 'Print…',
        disabled: !mine,
        onSelect: tryTo('print it', async () => {
          setNotice(`Laying out “${work.title}” for the printer…`)
          const how = await printDraft(work.id)
          setNotice(how === 'saved' ? `Saved “${work.title}” as a PDF: open it to print.` : null)
        }),
      },
      {
        key: 'backup',
        label: 'Download a backup (.zip)',
        disabled: !mine,
        onSelect: tryTo('download it', () => downloadDraftExport(work.id, 'project', 'manuscript')),
      },
      { key: 'delete', label: 'Delete…', divider: true, disabled: !mine, onSelect: () => setDialog({ kind: 'delete', work }) },
    ]
  }

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
        <div className="library__actions">
          <button type="button" onClick={() => setImporting(true)}>Import…</button>
          <button type="button" onClick={() => setCreating((c) => !c)}>
            {creating ? 'Cancel' : 'Start something new'}
          </button>
        </div>
      </header>

      {importing && (
        <ImportDialog
          onClose={() => setImporting(false)}
          onBook={(book) => navigate(`/works/${book.id}`)}
        />
      )}

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
          <li
            key={work.id}
            className="library__item"
            onContextMenu={(e) => {
              e.preventDefault()
              setPointerMenu({ x: e.clientX, y: e.clientY, work })
            }}
          >
            {/* Beside the card rather than in it: a menu cannot sit inside a link. */}
            <Menu
              className="library__menu"
              label={<span aria-hidden="true">⋯</span>}
              ariaLabel={`Actions for ${work.title}`}
              title="Actions"
              caret={false}
              items={bookMenu(work)}
            />
            <Link to={`/works/${work.id}`} className="library__card">
              <h2>{work.title}</h2>
              <p className="library__meta">
                <span className={`badge badge--${work.status}`}>{work.status}</span>
                {user && work.ownerId !== user.id && <span className="badge">shared with you</span>}
                {KINDS.find((k) => k.value === work.kind)?.label} · {work.wordCount.toLocaleString()} words
              </p>
              {work.synopsis && <p className="library__synopsis">{work.synopsis}</p>}
              <p className="library__meta">Edited {new Date(work.updatedAt).toLocaleDateString()}</p>
            </Link>
          </li>
        ))}
      </ul>

      {pointerMenu && (
        <ContextMenu
          x={pointerMenu.x}
          y={pointerMenu.y}
          label={`Actions for ${pointerMenu.work.title}`}
          items={bookMenu(pointerMenu.work)}
          onClose={() => setPointerMenu(null)}
        />
      )}
      {dialog?.kind === 'rename' && (
        <RenameDialog
          title="Rename the book"
          initial={dialog.work.title}
          onSave={async (title) => {
            await api.works.update(dialog.work.id, { title })
            load()
          }}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === 'export' && <ExportDialog workId={dialog.work.id} onClose={() => setDialog(null)} />}
      {dialog?.kind === 'delete' && (
        <ConfirmDialog
          title={`Delete “${dialog.work.title}”?`}
          action="Delete book"
          confirmWord={dialog.work.title}
          onConfirm={async () => {
            await api.works.remove(dialog.work.id)
            setNotice(`Deleted “${dialog.work.title}”.`)
            load()
          }}
          onClose={() => setDialog(null)}
        >
          <p>
            The whole book goes, for good: its {dialog.work.wordCount.toLocaleString()} words, its chapters and
            notebook, every version, and anything published from it. It cannot be brought back.
          </p>
          <p>A backup first is one click: “Download a backup (.zip)” in this menu.</p>
        </ConfirmDialog>
      )}
      {notice && <div className="toast" role="status">{notice}</div>}
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
