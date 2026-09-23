import { useEffect, useState } from 'react'
import { api } from '../api/client'
import type { Scene } from '../api/types'

interface Props {
  scene: Scene
  onSaved: (scene: Scene) => void
  onSplit: () => void
  onMerge: () => void
  canMerge: boolean
}

/**
 * The card behind the scene: what it is about, who is in it, where and when.
 *
 * These are the fields the corkboard filters on, so they are worth filling in
 * even though none of them appear in the finished book.
 */
export function SceneDetails({ scene, onSaved, onSplit, onMerge, canMerge }: Props) {
  const [draft, setDraft] = useState({
    summary: scene.summary ?? '',
    pov: scene.pov ?? '',
    location: scene.location ?? '',
    storyDate: scene.storyDate ?? '',
  })
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)

  // A different scene means a different card.
  useEffect(() => {
    setDraft({
      summary: scene.summary ?? '',
      pov: scene.pov ?? '',
      location: scene.location ?? '',
      storyDate: scene.storyDate ?? '',
    })
    setSaved(false)
  }, [scene.id, scene.summary, scene.pov, scene.location, scene.storyDate])

  const field = (key: keyof typeof draft, value: string) => {
    setDraft((current) => ({ ...current, [key]: value }))
    setSaved(false)
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
      <label>
        Summary
        <textarea
          rows={3}
          value={draft.summary}
          placeholder="What happens, in one or two lines"
          onChange={(e) => field('summary', e.target.value)}
        />
      </label>
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
