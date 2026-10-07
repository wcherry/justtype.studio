import { useCallback, useEffect, useState } from 'react'

/**
 * How the page looks and behaves while writing.
 *
 * Per-device rather than per-account: the right measure and type size depend on
 * the screen in front of you, so carrying a laptop's settings onto a phone would
 * be actively wrong.
 */
export interface WritingPreferences {
  focusMode: boolean
  typewriter: boolean
  /** Line length in characters — the measure. */
  measure: number
  fontSize: number
  lineHeight: number
  family: 'serif' | 'sans' | 'mono'
  /** The writing room's zoom, in percent: the type and the page width together. */
  roomZoom: number
  /** How much of the screen's width the writing room's page takes, in percent. */
  roomWidth: number
}

export const DEFAULT_PREFERENCES: WritingPreferences = {
  focusMode: false,
  typewriter: false,
  measure: 34,
  fontSize: 18,
  lineHeight: 1.7,
  family: 'serif',
  roomZoom: 100,
  roomWidth: 40,
}

const KEY = 'justtype.writingPreferences'

function load(): WritingPreferences {
  try {
    const raw = localStorage.getItem(KEY)
    // Merged over the defaults so a preference added later does not arrive
    // undefined for everyone who already has settings stored.
    return raw ? { ...DEFAULT_PREFERENCES, ...(JSON.parse(raw) as Partial<WritingPreferences>) } : DEFAULT_PREFERENCES
  } catch {
    return DEFAULT_PREFERENCES
  }
}

export function useWritingPreferences() {
  const [preferences, setPreferences] = useState<WritingPreferences>(load)

  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(preferences))
    } catch {
      // Blocked site data: the session still works, it just starts from the
      // defaults next time.
    }
  }, [preferences])

  const update = useCallback(<K extends keyof WritingPreferences>(field: K, value: WritingPreferences[K]) => {
    setPreferences((current) => ({ ...current, [field]: value }))
  }, [])

  const reset = useCallback(() => setPreferences(DEFAULT_PREFERENCES), [])

  /** Fed to the prose container as inline custom properties. */
  const style: React.CSSProperties = {
    '--measure': `${preferences.measure}em`,
    '--prose-size': `${preferences.fontSize}px`,
    '--prose-leading': String(preferences.lineHeight),
    '--prose-family': `var(--family-${preferences.family})`,
  } as React.CSSProperties

  return { preferences, update, reset, style }
}
