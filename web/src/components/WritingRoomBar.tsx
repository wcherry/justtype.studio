import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from 'react'

import type { Outline, Scene } from '../api/types'
import { Menu } from './Menu'
import { ToolButton } from './Toolbar'
import type { MenuItem } from './Menu'

/** How long the bar stays down on the way in, before it gets out of the way. */
const SHOW_ON_ENTRY_MS = 3000
/** How long it waits once the pointer has left it, so a slip does not hide it. */
const HIDE_AFTER_LEAVING_MS = 700

export const ZOOM = { min: 25, max: 400, step: 5 } as const
/** What ⌘= and ⌘− move the zoom by. */
const ZOOM_KEY_STEP = 10
export const WIDTH = { min: 10, max: 100, step: 5 } as const

const clamp = (value: number, { min, max }: { min: number; max: number }) => Math.min(max, Math.max(min, value))

/**
 * The writing room's pull-down bar: where you are (and a way to go anywhere
 * else in the book), the zoom, the page width, the outline and the way out.
 *
 * Down for three seconds on the way in, then gone until the pointer comes
 * into the strip the bar would cover, from the top of the screen to where its
 * bottom edge sits. It stays while the pointer is over it or focus is in
 * it, so a menu opened from it never vanishes underneath you.
 */
export function WritingRoomBar({
  outline, scene = null, title, wordCount, saveLabel, zoom, onZoom, width, onWidth,
  outlineShown = false, onToggleOutline, outlinePanel, onOpenScene, onLeave,
}: {
  /** The book, for the menu of places to go. Without one (a Daily Prompts
   *  answer) the bar shows `title` and has no outline. */
  outline?: Outline
  scene?: Scene | null
  title?: string
  wordCount: number
  saveLabel: string
  zoom: number
  onZoom: (percent: number) => void
  /** How much of the screen's width the page takes, in percent. */
  width: number
  onWidth: (percent: number) => void
  outlineShown?: boolean
  onToggleOutline?: () => void
  /** The outline itself, shown in a popup under its button. */
  outlinePanel?: ReactNode
  onOpenScene?: (scene: Scene) => void
  onLeave: () => void
}) {
  const [shown, setShown] = useState(true)
  const hideTimer = useRef<number | undefined>(undefined)
  const over = useRef(false)
  const barRef = useRef<HTMLDivElement>(null)

  /** Brings the bar down, and takes it away again after `ms` unless it is in use. */
  const reveal = useCallback((ms: number) => {
    setShown(true)
    window.clearTimeout(hideTimer.current)
    hideTimer.current = window.setTimeout(() => {
      if (!over.current) setShown(false)
    }, ms)
  }, [])

  useEffect(() => {
    reveal(SHOW_ON_ENTRY_MS)
    // Measured each time: the bar wraps onto a second row on a narrow window.
    const underBar = (event: PointerEvent) => event.clientY <= (barRef.current?.offsetHeight ?? 40)
    const atTop = (event: PointerEvent) => {
      if (event.pointerType === 'mouse' && underBar(event)) reveal(HIDE_AFTER_LEAVING_MS * 2)
    }
    // Touch has no pointer resting anywhere: a tap in the same strip does it.
    const tapAtTop = (event: PointerEvent) => {
      if (event.pointerType !== 'mouse' && underBar(event)) reveal(SHOW_ON_ENTRY_MS)
    }
    document.addEventListener('pointermove', atTop)
    document.addEventListener('pointerdown', tapAtTop)
    return () => {
      document.removeEventListener('pointermove', atTop)
      document.removeEventListener('pointerdown', tapAtTop)
      window.clearTimeout(hideTimer.current)
    }
  }, [reveal])

  const outlineId = useId()
  const outlineRef = useRef<HTMLDivElement>(null)
  // The outline holds the bar down while it is open, and when it closes the
  // bar goes the usual way.
  useEffect(() => {
    if (outlineShown) {
      window.clearTimeout(hideTimer.current)
      setShown(true)
    } else if (!over.current) {
      reveal(HIDE_AFTER_LEAVING_MS)
    }
  }, [outlineShown, reveal])

  // A click anywhere else, or Escape, puts the outline away. Escape is taken
  // before the room sees it, so it closes the popup rather than the room.
  useEffect(() => {
    if (!outlineShown || !onToggleOutline) return
    const away = (event: PointerEvent) => {
      if (!outlineRef.current?.contains(event.target as Node)) onToggleOutline()
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || (event.target as HTMLElement).closest('input, textarea')) return
      event.preventDefault()
      onToggleOutline()
    }
    document.addEventListener('pointerdown', away)
    window.addEventListener('keydown', onKey, true)
    return () => {
      document.removeEventListener('pointerdown', away)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [outlineShown, onToggleOutline])

  const zoomTo = useCallback((percent: number) => onZoom(clamp(percent, ZOOM)), [onZoom])

  // ⌘= and ⌘− zoom the page here rather than the whole browser window, and
  // ⌘0 puts it back; the bar comes down so the new size is shown.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey) return
      let next: number | null = null
      if (event.key === '=' || event.key === '+') next = zoom + ZOOM_KEY_STEP
      else if (event.key === '-') next = zoom - ZOOM_KEY_STEP
      else if (event.key === '0') next = 100
      if (next === null) return
      event.preventDefault()
      zoomTo(next)
      reveal(SHOW_ON_ENTRY_MS / 2)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [zoom, zoomTo, reveal])

  const chapter = outline?.chapters.find((c) => c.id === scene?.chapterId)
  const places: MenuItem[] = (outline?.chapters ?? []).flatMap((c) => [
    { key: `chapter-${c.id}`, label: c.title, heading: true },
    ...(c.scenes.length === 0
      ? [{ key: `empty-${c.id}`, label: 'No scenes yet', disabled: true }]
      : c.scenes.map((s) => ({
          key: s.id,
          label: s.title,
          checked: s.id === scene?.id,
          onSelect: () => onOpenScene?.(s),
        }))),
  ])

  return (
    <div
      ref={barRef}
      className={shown ? 'room-bar is-shown' : 'room-bar'}
      role="toolbar"
      aria-label="Writing room"
      onPointerEnter={() => {
        over.current = true
        window.clearTimeout(hideTimer.current)
        setShown(true)
      }}
      onPointerLeave={() => {
        over.current = false
        if (!outlineShown) reveal(HIDE_AFTER_LEAVING_MS)
      }}
    >
      {outline ? (
        <Menu
          align="left"
          className="room-bar__where"
          title="Go to another scene"
          label={
            <span>
              {chapter && <span className="room-bar__chapter">{chapter.title} › </span>}
              <span className="room-bar__scene">
                {chapter?.scenes.find((s) => s.id === scene?.id)?.title ?? outline.title}
              </span>
            </span>
          }
          items={places}
        />
      ) : (
        <span className="room-bar__where">
          <span className="room-bar__scene">{title}</span>
        </span>
      )}
      <span className="room-bar__meta">
        {wordCount.toLocaleString()} words · {saveLabel || 'Saved'}
      </span>
      <span className="room-bar__spacer" />

      <Slider label="Zoom" value={zoom} range={ZOOM} onChange={zoomTo} title="Zoom (⌘=, ⌘−, ⌘0 for 100%)" />
      <Slider label="Width" value={width} range={WIDTH} onChange={onWidth} title="How much of the screen the page takes" />

      <div className="room-bar__actions">
        {outlinePanel && onToggleOutline && <div className="room-outline" ref={outlineRef}>
          <ToolButton
            icon="outline"
            label="Outline (⌘⇧O)"
            pressed={outlineShown}
            expanded={outlineShown}
            controls={outlineShown ? outlineId : undefined}
            onClick={onToggleOutline}
          />
          {outlineShown && (
            <div className="room-outline__popup" id={outlineId}>
              {outlinePanel}
            </div>
          )}
        </div>}
        <ToolButton icon="leave" label="Back to the desk (Esc)" onClick={onLeave} />
      </div>
    </div>
  )
}

/** A labelled slider with its value, as a percentage, in small type under it. */
function Slider({
  label, value, range, onChange, title,
}: {
  label: string
  value: number
  range: { min: number; max: number; step: number }
  onChange: (value: number) => void
  title: string
}) {
  return (
    <label className="room-slider" title={title}>
      <span className="room-slider__label">{label}</span>
      <span className="room-slider__control">
        <input
          type="range"
          min={range.min}
          max={range.max}
          step={range.step}
          value={value}
          aria-valuetext={`${value}%`}
          onChange={(event) => onChange(Number(event.target.value))}
        />
        <span className="room-slider__value" aria-hidden="true">{value}%</span>
      </span>
    </label>
  )
}
