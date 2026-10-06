import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import type { WritingSummary } from '../api/types'

/** `YYYY-MM-DD` in the writer's own timezone, which is the only one that matters. */
export function localDay(date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

/**
 * Today's words against a target, the streak, and words written in this
 * sitting.
 *
 * "This session" is counted on the client from the word count when the editor
 * opened, because a session is a browser-tab notion the server has no view of.
 * Today and the streak come from the server, so they survive a reload and
 * follow the writer between devices.
 */
/**
 * The baseline lives in sessionStorage, keyed by work.
 *
 * A plain ref would reset on any remount — a hot reload in development, a
 * StrictMode double-mount, or simply navigating away and back — and silently
 * restart the count at whatever the total happened to be, reporting zero
 * progress for a sitting that had plenty. sessionStorage is scoped to the tab
 * and cleared when it closes, which is exactly what "this sitting" means.
 */
function baselineFor(workId: string | null, current: number | null): number | null {
  if (workId === null || current === null) return null
  const key = `justtype.sessionStart.${workId}`
  try {
    const stored = sessionStorage.getItem(key)
    if (stored !== null) {
      const parsed = Number(stored)
      if (Number.isFinite(parsed)) return parsed
    }
    sessionStorage.setItem(key, String(current))
  } catch {
    // Blocked site data: fall back to "this render is the baseline", which
    // under-reports rather than inventing progress that did not happen.
  }
  return current
}

export function useSessionGoals(workWordCount: number | null, workId: string | null = null) {
  const [summary, setSummary] = useState<WritingSummary | null>(null)
  const [day, setDay] = useState(localDay)
  const sessionStart = useRef<number | null>(null)

  const resolved = baselineFor(workId, workWordCount)
  if (resolved !== null) sessionStart.current = resolved

  const refresh = useCallback(async () => {
    try {
      setSummary(await api.goals.writing(localDay()))
    } catch {
      // Offline, most likely. Goals are encouragement, not data to protect —
      // the queue is what protects the writing. Leave the last figures up.
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // A writer who works past midnight should roll over to a new day rather than
  // keep adding to yesterday.
  useEffect(() => {
    const timer = window.setInterval(() => {
      const today = localDay()
      if (today !== day) {
        setDay(today)
        void refresh()
      }
    }, 60_000)
    return () => window.clearInterval(timer)
  }, [day, refresh])

  const setDailyGoal = useCallback(
    async (goal: number | null) => {
      await api.goals.update({ dailyWordGoal: goal })
      await refresh()
    },
    [refresh],
  )

  const setWeeklyGoal = useCallback(
    async (goal: number | null) => {
      await api.goals.update({ weeklyWordGoal: goal })
      await refresh()
    },
    [refresh],
  )

  const sessionWords =
    workWordCount !== null && sessionStart.current !== null
      ? workWordCount - sessionStart.current
      : 0

  return { summary, sessionWords, refresh, setDailyGoal, setWeeklyGoal, day }
}
