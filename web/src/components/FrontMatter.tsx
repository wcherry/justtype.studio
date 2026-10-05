import { useEffect, useState } from 'react'
import { api } from '../api/client'
import type { Genre, Work } from '../api/types'
import { GENRES } from './labels'

interface Props {
  work: Work
  onSaved: (work: Work) => void
}

/**
 * The parts of a manuscript that are not the manuscript: who it is by, what it
 * says on the title page, and what an editor needs to put a dropped page back.
 *
 * Contact details live on the writer rather than here, because they are the
 * same for every submission.
 */
export function FrontMatter({ work, onSaved }: Props) {
  const [draft, setDraft] = useState({
    penName: work.penName ?? '',
    shortTitle: work.shortTitle ?? '',
    dedication: work.dedication ?? '',
    epigraph: work.epigraph ?? '',
    epigraphSource: work.epigraphSource ?? '',
    copyrightLine: work.copyrightLine ?? '',
    genre: (work.genre ?? '') as Genre | '',
  })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  useEffect(() => setSaved(false), [draft])

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      onSaved(await api.works.update(work.id, draft))
      setSaved(true)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save that')
    } finally {
      setBusy(false)
    }
  }

  const field = (key: keyof typeof draft, value: string) =>
    setDraft((current) => ({ ...current, [key]: value }))

  return (
    <form className="front-matter" onSubmit={submit}>
      <label>
        Genre
        <select value={draft.genre} onChange={(e) => field('genre', e.target.value)}>
          <option value="">Not sure yet</option>
          {GENRES.map((g) => (
            <option key={g.value} value={g.value}>{g.label}</option>
          ))}
        </select>
        <span className="field__hint">What readers browsing the review feed find it under.</span>
      </label>

      <label>
        Byline
        <input
          value={draft.penName}
          placeholder="your display name"
          onChange={(e) => field('penName', e.target.value)}
        />
        <span className="field__hint">A pen name, if this one is not under yours.</span>
      </label>

      <label>
        Short title
        <input
          value={draft.shortTitle}
          placeholder={work.title}
          onChange={(e) => field('shortTitle', e.target.value)}
        />
        <span className="field__hint">For the running header. A long title wraps and ruins it.</span>
      </label>

      <label>
        Dedication
        <textarea rows={2} value={draft.dedication} onChange={(e) => field('dedication', e.target.value)} />
      </label>

      <label>
        Epigraph
        <textarea rows={3} value={draft.epigraph} onChange={(e) => field('epigraph', e.target.value)} />
      </label>

      <label>
        Epigraph source
        <input value={draft.epigraphSource} onChange={(e) => field('epigraphSource', e.target.value)} />
      </label>

      <label>
        Copyright line
        <input
          value={draft.copyrightLine}
          placeholder="© 2026 Your Name"
          onChange={(e) => field('copyrightLine', e.target.value)}
        />
      </label>

      {error && <p className="error">{error}</p>}
      <button type="submit" disabled={busy}>{saved ? 'Saved' : 'Save front matter'}</button>
    </form>
  )
}
