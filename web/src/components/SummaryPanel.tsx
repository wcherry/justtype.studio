import { useState } from 'react'
import { ApiError, api } from '../api/client'
import type { Outline, Scene, SummaryView } from '../api/types'
import { useAuth } from '../state/auth'

type Target = 'scene' | 'chapter' | 'work'

const WHERE: Record<Target, string> = {
  scene: 'the scene’s summary',
  chapter: 'the chapter’s summary',
  work: 'the book’s synopsis',
}

/**
 * A summary of the scene, the chapter or the book, written by Claude.
 *
 * Off until the writer turns it on, and the panel says plainly what turning it
 * on means before they do. What comes back is a suggestion: it goes into the
 * summary or synopsis only when the writer puts it there.
 */
export function SummaryPanel({
  outline, scene, canUseSynopsis, onApplied,
}: {
  outline: Outline
  scene: Scene | null
  /** The synopsis is front matter, an owner's to change. */
  canUseSynopsis: boolean
  onApplied: () => void
}) {
  const { user, updateUser } = useAuth()
  const [busy, setBusy] = useState<Target | null>(null)
  const [result, setResult] = useState<{ target: Target; view: SummaryView } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [applied, setApplied] = useState(false)

  const chapter = scene ? outline.chapters.find((c) => c.id === scene.chapterId) ?? null : null
  const allowed = !!user?.summariesAllowedAt

  if (!allowed) {
    return (
      <div className="summaries">
        <p>
          Summaries are written by Claude, a model Anthropic runs. Turning them on means the
          text you ask to summarise — that scene, chapter or book, and only when you ask — is
          sent to Anthropic. The summary is a suggestion; nothing in your manuscript changes
          unless you put it there.
        </p>
        <p className="field__hint">You can turn them off again in your account at any time.</p>
        {error && <p className="error">{error}</p>}
        <button
          type="button"
          onClick={async () => {
            try {
              updateUser(await api.summaries.allow())
            } catch (e) {
              setError(e instanceof Error ? e.message : 'Could not turn them on')
            }
          }}
        >
          Turn on summaries
        </button>
      </div>
    )
  }

  const ask = async (target: Target) => {
    setBusy(target)
    setError(null)
    setApplied(false)
    try {
      const view =
        target === 'scene' && scene ? await api.summaries.scene(scene.id)
          : target === 'chapter' && chapter ? await api.summaries.chapter(chapter.id)
            : await api.summaries.work(outline.id)
      setResult({ target, view })
    } catch (e) {
      setError(
        e instanceof ApiError && e.status === 429
          ? e.message
          : e instanceof Error ? e.message : 'Could not summarise that',
      )
    } finally {
      setBusy(null)
    }
  }

  const apply = async () => {
    if (!result) return
    const text = result.view.summary
    try {
      if (result.target === 'scene' && scene) await api.scenes.update(scene.id, { summary: text })
      else if (result.target === 'chapter' && chapter) await api.chapters.update(chapter.id, { summary: text })
      else await api.works.update(outline.id, { synopsis: text })
      setApplied(true)
      onApplied()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not put it there')
    }
  }

  return (
    <div className="summaries">
      <div className="summaries__targets">
        <button type="button" disabled={!scene || busy !== null} onClick={() => void ask('scene')}>
          {busy === 'scene' ? 'Reading…' : 'This scene'}
        </button>
        <button type="button" disabled={!chapter || busy !== null} onClick={() => void ask('chapter')}>
          {busy === 'chapter' ? 'Reading…' : 'This chapter'}
        </button>
        <button type="button" disabled={busy !== null} onClick={() => void ask('work')}>
          {busy === 'work' ? 'Reading the book…' : 'The whole book'}
        </button>
      </div>
      {busy === 'work' && (
        <p className="field__hint">A whole book takes a minute or two to read.</p>
      )}
      {error && <p className="error">{error}</p>}

      {result && (
        <div className="summaries__result">
          <p className="summaries__text">{result.view.summary}</p>
          <p className="field__hint">
            {result.view.cached ? 'Written earlier for this same text' : 'Written just now'} by{' '}
            {result.view.model}.
          </p>
          {(result.target !== 'work' || canUseSynopsis) && (
            <button type="button" disabled={applied} onClick={() => void apply()}>
              {applied ? 'Done' : `Use as ${WHERE[result.target]}`}
            </button>
          )}
          {result.target === 'work' && !canUseSynopsis && (
            <p className="field__hint">The synopsis is the book owner’s to change.</p>
          )}
        </div>
      )}
    </div>
  )
}
