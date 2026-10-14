import { useMemo, useRef, useState } from 'react'
import type { Outline, Scene } from '../api/types'
import { statusOf, statusesOf } from '../statuses'

interface Props {
  outline: Outline
  activeSceneId: string | null
  onOpenScene: (scene: Scene) => void
  /** Reorder within a chapter, or move a scene into another one. */
  onMove: (scene: Scene, toChapterId: string, toIndex: number) => void
  /** Only these chapters, for a chapter or part looked at on its own. */
  only?: { chapterIds: string[]; label: string }
  /** Back to the whole book, when showing `only`. */
  onShowAll?: () => void
}

interface Filters {
  text: string
  pov: string
  location: string
  status: string
}

const EMPTY: Filters = { text: '', pov: '', location: '', status: '' }

/**
 * The whole work as index cards.
 *
 * Fiction gets planned at this altitude — a scene is a card with a person, a
 * place and a job to do — and the outline sidebar cannot show that while also
 * being narrow enough to write beside.
 *
 * Drag and drop is the plain HTML5 API rather than a library: the interaction
 * is one draggable element and one drop target, and a dependency for that is
 * more code than the thing it replaces.
 */
export function Corkboard({ outline, activeSceneId, onOpenScene, onMove, only, onShowAll }: Props) {
  const statuses = statusesOf(outline)
  const chapters = only ? outline.chapters.filter((c) => only.chapterIds.includes(c.id)) : outline.chapters
  const [filters, setFilters] = useState<Filters>(EMPTY)
  // The dragged scene lives in a ref, not just state: `drop` reads it, and a
  // state value would still be the previous render's when both events land in
  // one tick. State is kept alongside purely so the card can look dragged.
  const dragged = useRef<Scene | null>(null)
  const [dragging, setDragging] = useState<Scene | null>(null)
  const [over, setOver] = useState<string | null>(null)

  const startDrag = (scene: Scene) => {
    dragged.current = scene
    setDragging(scene)
  }

  const endDrag = () => {
    dragged.current = null
    setDragging(null)
    setOver(null)
  }

  // Offered rather than typed: the values that exist are the useful ones, and
  // a writer should not have to remember how they spelled a location.
  const { povs, locations } = useMemo(() => {
    const scenes = chapters.flatMap((c) => c.scenes)
    const unique = (values: (string | null | undefined)[]) =>
      [...new Set(values.filter((v): v is string => !!v && v.trim() !== ''))].sort()
    return { povs: unique(scenes.map((s) => s.pov)), locations: unique(scenes.map((s) => s.location)) }
  }, [outline, only]) // eslint-disable-line react-hooks/exhaustive-deps

  const matches = (scene: Scene) => {
    const text = filters.text.trim().toLowerCase()
    if (text) {
      const haystack = [scene.title, scene.summary, scene.pov, scene.location, scene.storyDate]
        .filter(Boolean)
        .join(' ')
        .toLowerCase()
      if (!haystack.includes(text)) return false
    }
    if (filters.pov && scene.pov !== filters.pov) return false
    if (filters.location && scene.location !== filters.location) return false
    if (filters.status && scene.status !== filters.status) return false
    return true
  }

  const filtering = JSON.stringify(filters) !== JSON.stringify(EMPTY)
  const visible = chapters.flatMap((c) => c.scenes).filter(matches).length
  const total = chapters.flatMap((c) => c.scenes).length

  const drop = (chapterId: string, index: number) => {
    const scene = dragged.current
    if (scene) onMove(scene, chapterId, index)
    endDrag()
  }

  return (
    <div className="corkboard">
      {only && (
        <p className="corkboard__only">
          Showing {only.label}
          {onShowAll && <button type="button" className="linklike" onClick={onShowAll}>Show the whole book</button>}
        </p>
      )}
      <header className="corkboard__filters">
        <input
          type="search"
          placeholder="Search titles, summaries, POV, places…"
          value={filters.text}
          onChange={(e) => setFilters({ ...filters, text: e.target.value })}
        />
        <select value={filters.pov} onChange={(e) => setFilters({ ...filters, pov: e.target.value })}>
          <option value="">Any POV</option>
          {povs.map((pov) => <option key={pov} value={pov}>{pov}</option>)}
        </select>
        <select value={filters.location} onChange={(e) => setFilters({ ...filters, location: e.target.value })}>
          <option value="">Anywhere</option>
          {locations.map((place) => <option key={place} value={place}>{place}</option>)}
        </select>
        <select value={filters.status} onChange={(e) => setFilters({ ...filters, status: e.target.value })}>
          <option value="">Any status</option>
          {statuses.map((status) => <option key={status.key} value={status.key}>{status.label}</option>)}
        </select>
        {filtering && (
          <>
            <span className="corkboard__count">{visible} of {total}</span>
            <button type="button" className="linklike" onClick={() => setFilters(EMPTY)}>Clear</button>
          </>
        )}
      </header>

      {chapters.map((chapter) => {
        const scenes = chapter.scenes.filter(matches)
        return (
          <section key={chapter.id} className="corkboard__chapter">
            <h3>
              {chapter.title}
              <span className="corkboard__meta">
                {chapter.scenes.reduce((sum, s) => sum + s.wordCount, 0).toLocaleString()} words
              </span>
            </h3>

            <div className="corkboard__cards">
              {scenes.map((scene, index) => (
                <article
                  key={scene.id}
                  className={[
                    'card',
                    scene.id === activeSceneId ? 'is-active' : '',
                    dragging?.id === scene.id ? 'is-dragging' : '',
                    over === scene.id ? 'is-over' : '',
                  ].filter(Boolean).join(' ')}
                  style={{ borderLeftColor: statusOf(statuses, scene.status).color }}
                  title={statusOf(statuses, scene.status).label}
                  draggable
                  onDragStart={() => startDrag(scene)}
                  onDragEnd={endDrag}
                  onDragOver={(e) => { e.preventDefault(); setOver(scene.id) }}
                  onDragLeave={() => setOver((current) => (current === scene.id ? null : current))}
                  onDrop={(e) => { e.preventDefault(); drop(chapter.id, index) }}
                  onDoubleClick={() => onOpenScene(scene)}
                >
                  <h4>{scene.title}</h4>
                  {scene.summary
                    ? <p className="card__summary">{scene.summary}</p>
                    : <p className="card__summary card__summary--empty">No summary yet.</p>}
                  <footer className="card__meta">
                    {scene.pov && <span title="Point of view">{scene.pov}</span>}
                    {scene.location && <span title="Location">{scene.location}</span>}
                    {scene.storyDate && <span title="When, in the story">{scene.storyDate}</span>}
                    <span className="card__words">{scene.wordCount.toLocaleString()}</span>
                  </footer>
                </article>
              ))}

              {/* A target at the end, so a scene can be dropped into an empty
                  chapter or moved past the last card. */}
              <div
                className={over === `end-${chapter.id}` ? 'card card--drop is-over' : 'card card--drop'}
                onDragOver={(e) => { e.preventDefault(); setOver(`end-${chapter.id}`) }}
                onDragLeave={() => setOver((c) => (c === `end-${chapter.id}` ? null : c))}
                onDrop={(e) => { e.preventDefault(); drop(chapter.id, chapter.scenes.length) }}
              >
                {scenes.length === 0 && !filtering ? 'Drop a scene here' : 'Move to end'}
              </div>
            </div>
          </section>
        )
      })}

      <p className="corkboard__hint">Drag a card to reorder it, or onto another chapter to move it. Double-click to open it.</p>
    </div>
  )
}
