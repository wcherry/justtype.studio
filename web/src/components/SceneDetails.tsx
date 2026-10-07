import { useEffect, useState } from 'react'
import { ApiError, api } from '../api/client'
import type { Scene } from '../api/types'
import { useAuth } from '../state/auth'

interface Props {
  scene: Scene
  onSaved: (scene: Scene) => void
  onSplit: () => void
  onMerge: () => void
  canMerge: boolean
  /** The server can write summaries, and this writer may change the scene. */
  canGenerate?: boolean
  /** Lands any unsaved typing first, so the summary is of the words on the page. */
  beforeGenerate?: () => Promise<unknown>
}

/**
 * The card behind the scene: what it is about, who is in it, where and when.
 *
 * These are the fields the corkboard filters on, so they are worth filling in
 * even though none of them appear in the finished book.
 */
export function SceneDetails({
  scene, onSaved, onSplit, onMerge, canMerge, canGenerate = false, beforeGenerate,
}: Props) {
  const [draft, setDraft] = useState({
    summary: scene.summary ?? '',
    pov: scene.pov ?? '',
    location: scene.location ?? '',
    storyDate: scene.storyDate ?? '',
  })
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)
  const { user, updateUser } = useAuth()
  const [generating, setGenerating] = useState(false)
  const [asking, setAsking] = useState(false)
  const [generateError, setGenerateError] = useState<string | null>(null)
  /** What the summary said before one was written for it, to put back. */
  const [replaced, setReplaced] = useState<string | null>(null)

  // A different scene means a different card.
  useEffect(() => {
    setDraft({
      summary: scene.summary ?? '',
      pov: scene.pov ?? '',
      location: scene.location ?? '',
      storyDate: scene.storyDate ?? '',
    })
    setSaved(false)
    setReplaced(null)
    setAsking(false)
    setGenerateError(null)
  }, [scene.id, scene.summary, scene.pov, scene.location, scene.storyDate])

  const field = (key: keyof typeof draft, value: string) => {
    setDraft((current) => ({ ...current, [key]: value }))
    setSaved(false)
  }

  /**
   * Asks Claude for a summary of the scene and puts it in the field — a draft,
   * like anything typed there: it is kept only when the writer saves.
   */
  const generate = async () => {
    setGenerateError(null)
    setGenerating(true)
    try {
      await beforeGenerate?.()
      const view = await api.summaries.scene(scene.id)
      setReplaced(draft.summary.trim() ? draft.summary : null)
      field('summary', view.summary)
    } catch (e) {
      setGenerateError(
        e instanceof ApiError && e.status === 429
          ? e.message
          : e instanceof Error ? e.message : 'Could not write a summary',
      )
    } finally {
      setGenerating(false)
    }
  }

  return (
    <form
      className="scene-details"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        try {
          onSaved(await api.scenes.update(scene.id, draft))
          setSaved(true)
        } finally {
          setBusy(false)
        }
      }}
    >
      <div className="scene-details__summary-head">
        <label htmlFor={`summary-${scene.id}`}>Summary</label>
        {canGenerate && (
          <button
            type="button"
            className="scene-details__generate"
            disabled={generating}
            title="Have Claude read the scene and write a summary for you to keep or change"
            onClick={() => (user?.summariesAllowedAt ? void generate() : setAsking(true))}
          >
            {generating ? 'Reading…' : 'AI Generate'}
          </button>
        )}
      </div>
      {asking && !user?.summariesAllowedAt && (
        <div className="scene-details__consent">
          <p>
            Summaries are written by Claude, a model Anthropic runs. Turning them on means the
            text of a scene is sent to Anthropic when you ask for its summary, and only then.
            You can turn them off again in your account.
          </p>
          <div className="scene-details__consent-actions">
            <button
              type="button"
              onClick={async () => {
                try {
                  updateUser(await api.summaries.allow())
                  setAsking(false)
                  void generate()
                } catch (e) {
                  setGenerateError(e instanceof Error ? e.message : 'Could not turn them on')
                }
              }}
            >
              Turn on and write one
            </button>
            <button type="button" className="linklike" onClick={() => setAsking(false)}>
              Not now
            </button>
          </div>
        </div>
      )}
      <textarea
        id={`summary-${scene.id}`}
        rows={3}
        value={draft.summary}
        placeholder="What happens, in one or two lines"
        onChange={(e) => {
          setReplaced(null)
          field('summary', e.target.value)
        }}
      />
      {generateError && <p className="error">{generateError}</p>}
      {replaced !== null && (
        <p className="field__hint">
          Written by Claude — save to keep it.{' '}
          <button
            type="button"
            className="linklike"
            onClick={() => {
              field('summary', replaced)
              setReplaced(null)
            }}
          >
            Put back what was there
          </button>
        </p>
      )}
      <label>
        Point of view
        <input value={draft.pov} onChange={(e) => field('pov', e.target.value)} />
      </label>
      <label>
        Location
        <input value={draft.location} onChange={(e) => field('location', e.target.value)} />
      </label>
      <label>
        When
        <input
          value={draft.storyDate}
          placeholder="The third day of the siege"
          onChange={(e) => field('storyDate', e.target.value)}
        />
        <span className="field__hint">Free text — fiction rarely runs on a calendar.</span>
      </label>

      <button type="submit" disabled={busy}>{saved ? 'Saved' : 'Save details'}</button>

      <div className="scene-details__structure">
        <h4>Structure</h4>
        <button type="button" onClick={onSplit}>Split at the cursor</button>
        <button type="button" onClick={onMerge} disabled={!canMerge}>
          Merge with the next scene
        </button>
        {!canMerge && <span className="field__hint">This is the last scene in its chapter.</span>}
      </div>
    </form>
  )
}
