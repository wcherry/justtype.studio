import { useEffect, useId, useRef, useState } from 'react'
import { Icon } from './Icon'
import { READER_LIMITS } from '../reader/readerPreferences'
import type { ReaderPreferences } from '../reader/readerPreferences'

interface Props {
  preferences: ReaderPreferences
  onChange: <K extends keyof ReaderPreferences>(field: K, value: ReaderPreferences[K]) => void
  onReset: () => void
}

/**
 * The reader's text controls: the column's width, the type's size and leading,
 * and whether notes and misspellings are underlined. Behind one icon, since a
 * reader sets them once and then reads.
 */
export function ReaderDisplay({ preferences, onChange, onReset }: Props) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const id = useId()

  useEffect(() => {
    if (!open) return
    const away = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false)
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      setOpen(false)
      buttonRef.current?.focus()
    }
    document.addEventListener('pointerdown', away)
    document.addEventListener('keydown', escape)
    return () => {
      document.removeEventListener('pointerdown', away)
      document.removeEventListener('keydown', escape)
    }
  }, [open])

  const range = (field: 'measure' | 'fontSize' | 'lineHeight') => (
    <input
      type="range"
      {...READER_LIMITS[field]}
      value={preferences[field]}
      onChange={(e) => onChange(field, Number(e.target.value))}
    />
  )

  return (
    <div className="reader-display" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        className={`reader-display__btn has-tip tip-end${open ? ' is-active' : ''}`}
        aria-label="Text settings"
        data-tip="Text settings"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((v) => !v)}
      >
        <Icon name="text" />
      </button>
      {open && (
        <div className="reader-display__panel prefs" id={id} role="group" aria-label="Text settings">
          <label>
            Width <span className="prefs__value">{preferences.measure} characters</span>
            {range('measure')}
          </label>
          <label>
            Size <span className="prefs__value">{preferences.fontSize}px</span>
            {range('fontSize')}
          </label>
          <label>
            Line spacing <span className="prefs__value">{preferences.lineHeight.toFixed(1)}</span>
            {range('lineHeight')}
          </label>
          <label className="prefs__toggle">
            <input
              type="checkbox"
              checked={preferences.showNotes}
              onChange={(e) => onChange('showNotes', e.target.checked)}
            />
            Underline notes
            <span className="prefs__hint">Mark the passages readers have commented on</span>
          </label>
          <label className="prefs__toggle">
            <input
              type="checkbox"
              checked={preferences.showSpelling}
              onChange={(e) => onChange('showSpelling', e.target.checked)}
            />
            Underline spelling mistakes
            <span className="prefs__hint">Words the spellchecker does not know</span>
          </label>
          <button type="button" className="linklike" onClick={onReset}>Reset to defaults</button>
        </div>
      )}
    </div>
  )
}
