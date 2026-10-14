import { useCallback, useEffect, useState } from 'react'

/**
 * How a manuscript looks while it is being read for review.
 *
 * Kept apart from the writing preferences: a writer who drafts in a narrow
 * column at 20px may well want to read someone else's book wider and smaller,
 * and a reader who is not a writer here has no writing preferences at all.
 * Per-device, for the same reason as those: the right size depends on the
 * screen in front of you.
 */
export interface ReaderPreferences {
  /** Line length in characters — the measure. */
  measure: number
  fontSize: number
  lineHeight: number
  /** Underline the passages that have notes on them. */
  showNotes: boolean
  /** Underline words the spellchecker does not know. */
  showSpelling: boolean
}

export const DEFAULT_READER_PREFERENCES: ReaderPreferences = {
  measure: 34,
  fontSize: 18,
  lineHeight: 1.7,
  showNotes: true,
  // A reviewer is reading, not proofing; typos are there for those who ask.
  showSpelling: false,
}

export const READER_LIMITS = {
  measure: { min: 24, max: 72, step: 1 },
  fontSize: { min: 14, max: 28, step: 1 },
  lineHeight: { min: 1.2, max: 2.4, step: 0.1 },
} as const

const KEY = 'justtype.readerPreferences'

function load(): ReaderPreferences {
  try {
    const raw = localStorage.getItem(KEY)
    return raw ? { ...DEFAULT_READER_PREFERENCES, ...(JSON.parse(raw) as Partial<ReaderPreferences>) } : DEFAULT_READER_PREFERENCES
  } catch {
    return DEFAULT_READER_PREFERENCES
  }
}

export function useReaderPreferences() {
  const [preferences, setPreferences] = useState<ReaderPreferences>(load)

  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(preferences))
    } catch {
      // Blocked site data: this visit still works, the next starts from the defaults.
    }
  }, [preferences])

  const update = useCallback(<K extends keyof ReaderPreferences>(field: K, value: ReaderPreferences[K]) => {
    setPreferences((current) => ({ ...current, [field]: value }))
  }, [])

  const reset = useCallback(() => setPreferences(DEFAULT_READER_PREFERENCES), [])

  // The width is in pixels rather than ems so the headings, which are set in a
  // smaller size than the prose, line up with the column the prose makes.
  const width = `${preferences.measure * preferences.fontSize}px`
  const style = {
    '--measure': width,
    '--reader-width': width,
    '--prose-size': `${preferences.fontSize}px`,
    '--prose-leading': String(preferences.lineHeight),
  } as React.CSSProperties

  return { preferences, update, reset, style }
}
