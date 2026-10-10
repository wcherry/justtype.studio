import { useEffect, useState } from 'react'
import { ApiError, bookDetails } from '../api/client'
import type { ContributorBody, MetadataResponse } from '../api/types'
import { uiLanguage } from '../languages'
import { LanguageInput } from './LanguageInput'

interface Props {
  workId: string
}

const ROLES: ContributorBody['role'][] = ['editor', 'translator', 'illustrator', 'co-author']

/**
 * What a book says about itself when it is published: its language, a
 * description, who else worked on it, and its cover. These end up in the
 * EPUB, the web page and the Word file's properties; a book in French that
 * says it is English is read in the wrong voice by a screen reader.
 *
 * Nothing here is required. A cover is the only one that shows.
 */
export function BookDetails({ workId }: Props) {
  const [details, setDetails] = useState<MetadataResponse | null>(null)
  const [draft, setDraft] = useState({
    language: '',
    textDirection: '',
    description: '',
    publisher: '',
    published: '',
    identifier: '',
    keywords: '',
  })
  const [contributors, setContributors] = useState<ContributorBody[]>([])
  const [cover, setCover] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  const show = (d: MetadataResponse) => {
    setDetails(d)
    setDraft({
      // A book with no language yet is offered the writer's own; it is saved
      // with the rest of the form.
      language: d.language ?? uiLanguage(),
      textDirection: d.textDirection ?? '',
      description: d.description ?? '',
      publisher: d.publisher ?? '',
      published: d.published ?? '',
      identifier: d.identifier ?? '',
      keywords: d.keywords.join(', '),
    })
    setContributors(d.contributors)
  }

  useEffect(() => {
    let live = true
    void bookDetails.get(workId).then((d) => live && show(d)).catch(() => undefined)
    return () => {
      live = false
    }
  }, [workId])

  // The cover is behind the token, so it is fetched and shown from memory.
  useEffect(() => {
    if (!details?.hasCover) {
      setCover(null)
      return
    }
    let url: string | null = null
    let live = true
    void bookDetails.coverUrl(workId).then((u) => {
      if (live) setCover((url = u))
      else URL.revokeObjectURL(u)
    }).catch(() => undefined)
    return () => {
      live = false
      if (url) URL.revokeObjectURL(url)
    }
  }, [workId, details?.hasCover, details?.coverBytes, details?.updatedAt])

  const field = (key: keyof typeof draft, value: string) => {
    setDraft((d) => ({ ...d, [key]: value }))
    setSaved(false)
  }

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      show(
        await bookDetails.save(workId, {
          ...draft,
          keywords: draft.keywords.split(',').map((k) => k.trim()).filter(Boolean),
          contributors: contributors.filter((c) => c.name.trim()),
        }),
      )
      setSaved(true)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not save that')
    } finally {
      setBusy(false)
    }
  }

  const upload = async (file: File | undefined) => {
    if (!file) return
    setBusy(true)
    setError(null)
    try {
      show(await bookDetails.setCover(workId, file))
    } catch (e) {
      setError(e instanceof ApiError || e instanceof Error ? e.message : 'Could not use that image')
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className="front-matter book-details" onSubmit={submit}>
      <h3>Publishing details</h3>
      <p className="field__hint">Used by EPUB, web and Word exports. None of it is required.</p>

      <div className="book-details__cover">
        {cover ? <img src={cover} alt="The cover" /> : <div className="book-details__nocover">No cover</div>}
        <div>
          <label className="book-details__upload">
            {details?.hasCover ? 'Replace the cover…' : 'Add a cover…'}
            <input
              type="file"
              accept="image/jpeg,image/png"
              disabled={busy}
              onChange={(e) => {
                void upload(e.target.files?.[0])
                e.target.value = ''
              }}
            />
          </label>
          {details?.hasCover && (
            <button
              type="button"
              disabled={busy}
              onClick={async () => show(await bookDetails.removeCover(workId))}
            >
              Remove
            </button>
          )}
          <span className="field__hint">
            JPEG or PNG, at least 1600 × 2560.
            {details?.coverWidth && <> This one is {details.coverWidth} × {details.coverHeight}.</>}
          </span>
          {details?.coverWarning && <span className="field__hint error">{details.coverWarning}</span>}
        </div>
      </div>

      <label>
        Language
        <LanguageInput value={draft.language} placeholder="en" onChange={(v) => field('language', v)} />
        <span className="field__hint">
          {details && !details.language && 'Not saved yet: this is your browser\'s language. '}
          Type a name or a code such as fr, pt-BR or he. It sets hyphenation and the reading voice.
        </span>
      </label>
      <label>
        Reads
        <select value={draft.textDirection} onChange={(e) => field('textDirection', e.target.value)}>
          <option value="">As the language does</option>
          <option value="ltr">Left to right</option>
          <option value="rtl">Right to left</option>
        </select>
      </label>
      <label>
        Description
        <textarea rows={4} value={draft.description} onChange={(e) => field('description', e.target.value)} />
        <span className="field__hint">The blurb a store or an e-reader shows.</span>
      </label>
      <label>
        Publisher
        <input value={draft.publisher} onChange={(e) => field('publisher', e.target.value)} />
      </label>
      <label>
        Publication date
        <input value={draft.published} placeholder="2026-03-01" onChange={(e) => field('published', e.target.value)} />
      </label>
      <label>
        Identifier
        <input value={draft.identifier} placeholder="ISBN, if you have one" onChange={(e) => field('identifier', e.target.value)} />
        <span className="field__hint">Optional. Never required.</span>
      </label>
      <label>
        Keywords
        <input value={draft.keywords} placeholder="sea, family, grief" onChange={(e) => field('keywords', e.target.value)} />
      </label>

      <fieldset className="book-details__people">
        <legend>Others who worked on it</legend>
        {contributors.map((c, i) => (
          <div key={i} className="book-details__person">
            <input
              aria-label="Name"
              value={c.name}
              onChange={(e) => {
                setContributors((list) => list.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))
                setSaved(false)
              }}
            />
            <select
              aria-label="Role"
              value={c.role}
              onChange={(e) => {
                setContributors((list) => list.map((x, j) => (j === i ? { ...x, role: e.target.value } : x)))
                setSaved(false)
              }}
            >
              {ROLES.map((r) => <option key={r} value={r}>{r}</option>)}
            </select>
            <button
              type="button"
              aria-label="Remove"
              onClick={() => {
                setContributors((list) => list.filter((_, j) => j !== i))
                setSaved(false)
              }}
            >
              ×
            </button>
          </div>
        ))}
        <button
          type="button"
          onClick={() => {
            setContributors((list) => [...list, { name: '', role: 'editor' }])
            setSaved(false)
          }}
        >
          + Add someone
        </button>
      </fieldset>

      {error && <p className="error">{error}</p>}
      <button type="submit" disabled={busy}>{saved ? 'Saved' : 'Save details'}</button>
    </form>
  )
}
