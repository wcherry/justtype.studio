import { useSyncExternalStore } from 'react'
import type { Theme } from '../api/types'

/**
 * How the app looks: the writer's choice, kept with their account so it
 * follows them to every device, and copied here so the page can be painted
 * in it before anything has loaded — and offline.
 *
 * The choice is applied as `data-theme` on <html>, which the tokens in
 * app.css key off. "system" is resolved here to light or dark, and again
 * whenever the device changes, so the stylesheet never has to ask.
 *
 * index.html repeats the first-paint half of this (read the copy, resolve,
 * set the attribute) as an inline script, because by the time this module
 * runs the page has already been painted once. Keep the two in step.
 */

const KEY = 'justtype.theme'

export const THEMES: { value: Theme; label: string; hint: string }[] = [
  { value: 'system', label: 'System', hint: 'Light or dark, following this device' },
  { value: 'light', label: 'Light', hint: 'Warm white paper' },
  { value: 'dark', label: 'Dark', hint: 'Light text on dark' },
  { value: 'sepia', label: 'Sepia', hint: 'Low-glare, for long days' },
  { value: 'night', label: 'Night', hint: 'Dim and quiet, for writing in the dark' },
]

const VALUES = new Set<string>(THEMES.map((t) => t.value))

function stored(): Theme {
  try {
    const value = localStorage.getItem(KEY)
    if (value && VALUES.has(value)) return value as Theme
  } catch {
    // Blocked site data: follow the device.
  }
  return 'system'
}

const dark = () =>
  typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches

function resolve(choice: Theme): Exclude<Theme, 'system'> {
  if (choice !== 'system') return choice
  return dark() ? 'dark' : 'light'
}

let current: Theme = stored()
const listeners = new Set<() => void>()

function paint() {
  document.documentElement.setAttribute('data-theme', resolve(current))
}

/** Shows `choice` now, and remembers it on this device for the next first paint. */
export function applyTheme(choice: Theme) {
  if (!VALUES.has(choice)) return
  try {
    localStorage.setItem(KEY, choice)
  } catch {
    // Still applied for this visit; the account keeps the real copy.
  }
  if (choice === current) return
  current = choice
  paint()
  for (const listener of listeners) listener()
}

if (typeof window !== 'undefined') {
  // The inline script has usually done this already; doing it again is
  // harmless, and covers a page where it could not run.
  paint()
  window.matchMedia?.('(prefers-color-scheme: dark)').addEventListener?.('change', () => {
    if (current === 'system') paint()
  })
}

/** The writer's current choice — "system" included, not what it resolves to. */
export function useTheme(): Theme {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    () => current,
  )
}
