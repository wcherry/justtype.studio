import { forwardRef, useEffect, useImperativeHandle, useRef } from 'react'

/**
 * Cloudflare Turnstile, the check that a person is signing up. Shown only
 * when the server hands out a site key (`GET /auth/signup-policy`), so
 * turning it on or off needs no rebuild. Its token is good for one sign-up:
 * after any refusal the form calls `reset()` for a fresh one.
 */

interface TurnstileApi {
  render: (el: HTMLElement, options: Record<string, unknown>) => string
  reset: (id: string) => void
  remove: (id: string) => void
}

declare global {
  interface Window {
    turnstile?: TurnstileApi
  }
}

const SCRIPT = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'
let loading: Promise<TurnstileApi> | null = null

function loadTurnstile(): Promise<TurnstileApi> {
  if (window.turnstile) return Promise.resolve(window.turnstile)
  loading ??= new Promise<TurnstileApi>((resolve, reject) => {
    const script = document.createElement('script')
    script.src = SCRIPT
    script.async = true
    script.onload = () => (window.turnstile ? resolve(window.turnstile) : reject(new Error('Turnstile did not load')))
    script.onerror = () => {
      loading = null
      reject(new Error('Turnstile did not load'))
    }
    document.head.appendChild(script)
  })
  return loading
}

export interface TurnstileHandle {
  reset: () => void
}

interface Props {
  siteKey: string
  /** A token once the check passes; `null` when it expires or fails. */
  onToken: (token: string | null) => void
  /** The check could not be shown at all (blocked script, offline). */
  onUnavailable?: () => void
}

export const TurnstileWidget = forwardRef<TurnstileHandle, Props>(function TurnstileWidget(
  { siteKey, onToken, onUnavailable },
  ref,
) {
  const box = useRef<HTMLDivElement>(null)
  const widget = useRef<string | null>(null)
  // The latest callbacks, so the widget is not rebuilt when they change.
  const callbacks = useRef({ onToken, onUnavailable })
  callbacks.current = { onToken, onUnavailable }

  useImperativeHandle(ref, () => ({
    reset: () => {
      callbacks.current.onToken(null)
      if (widget.current && window.turnstile) window.turnstile.reset(widget.current)
    },
  }), [])

  useEffect(() => {
    let gone = false
    loadTurnstile()
      .then((turnstile) => {
        if (gone || !box.current) return
        widget.current = turnstile.render(box.current, {
          sitekey: siteKey,
          action: 'signup',
          callback: (token: string) => callbacks.current.onToken(token),
          'expired-callback': () => callbacks.current.onToken(null),
          'error-callback': () => callbacks.current.onToken(null),
        })
      })
      .catch(() => {
        if (!gone) callbacks.current.onUnavailable?.()
      })
    return () => {
      gone = true
      if (widget.current && window.turnstile) window.turnstile.remove(widget.current)
      widget.current = null
    }
  }, [siteKey])

  return <div ref={box} className="auth__captcha" />
})
