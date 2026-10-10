import { useCallback, useEffect, useState } from 'react'
import { useLocation } from 'react-router-dom'
import { inbox } from '../api/client'

const CHANGED = 'justtype:inbox-changed'

/** After reading something, so the bell does not wait for the next navigation. */
export function notifyInboxChanged() {
  window.dispatchEvent(new Event(CHANGED))
}

/**
 * The bell's number. There is no live channel to the writer yet (that needs a
 * per-user room), so it is fetched on every navigation and whenever the
 * window comes back into focus — enough to be right when it is looked at.
 */
export function useUnreadCount(enabled: boolean): number {
  const [count, setCount] = useState(0)
  const location = useLocation()
  const refresh = useCallback(() => {
    if (!enabled) return
    inbox.unread().then((u) => setCount(u.count)).catch(() => undefined)
  }, [enabled])

  useEffect(() => {
    refresh()
  }, [refresh, location.pathname])

  useEffect(() => {
    const onFocus = () => refresh()
    const onVisible = () => document.visibilityState === 'visible' && refresh()
    window.addEventListener('focus', onFocus)
    window.addEventListener(CHANGED, onFocus)
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      window.removeEventListener('focus', onFocus)
      window.removeEventListener(CHANGED, onFocus)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [refresh])

  return enabled ? count : 0
}
