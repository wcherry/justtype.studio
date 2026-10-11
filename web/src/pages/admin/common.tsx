import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { ApiError } from '../../api/client'

export function errorText(e: unknown): string {
  if (e instanceof ApiError) return e.message
  return e instanceof Error ? e.message : 'Could not do that'
}

/** "14 Oct 2026, 09:30". */
export function when(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return Number.isNaN(d.getTime())
    ? iso
    : d.toLocaleString(undefined, { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' })
}

export function day(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })
}

/** Loads something and keeps it, with a way to load it again. */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [forbidden, setForbidden] = useState(false)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const run = useCallback(load, deps)
  const reload = useCallback(() => {
    setError(null)
    return run()
      .then((value) => {
        setData(value)
        return value
      })
      .catch((e) => {
        if (e instanceof ApiError && e.status === 403) setForbidden(true)
        setError(errorText(e))
        return null
      })
  }, [run])
  useEffect(() => {
    void reload()
  }, [reload])
  return { data, error, forbidden, reload, setData }
}

export function Pill({ children, tone = 'plain' }: { children: ReactNode; tone?: 'plain' | 'warn' | 'bad' | 'good' }) {
  return <span className={`pill pill--${tone}`}>{children}</span>
}

export function StatusPill({ status, ends }: { status: string; ends?: string | null }) {
  const tone = status === 'banned' ? 'bad' : status === 'restricted' || status === 'deactivated' ? 'warn' : status === 'active' ? 'good' : 'plain'
  return (
    <Pill tone={tone}>
      {status}
      {ends ? ` until ${day(ends)}` : ''}
    </Pill>
  )
}

/**
 * A button that opens a small form asking for a reason before it acts. Staff
 * actions are recorded with a reason, so none of them is one click.
 */
export function ReasonButton({
  label,
  prompt = 'Why? This goes in the log.',
  confirm = label,
  danger,
  extra,
  onDo,
}: {
  label: string
  prompt?: string
  confirm?: string
  danger?: boolean
  extra?: ReactNode
  onDo: (reason: string) => Promise<unknown>
}) {
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  if (!open) {
    return (
      <button type="button" className={danger ? 'is-danger' : undefined} onClick={() => setOpen(true)}>
        {label}
      </button>
    )
  }
  return (
    <form
      className="admin__reason"
      onSubmit={async (event) => {
        event.preventDefault()
        setBusy(true)
        setError(null)
        try {
          await onDo(reason.trim())
          setOpen(false)
          setReason('')
        } catch (e) {
          setError(errorText(e))
        } finally {
          setBusy(false)
        }
      }}
    >
      <label>
        {prompt}
        <textarea rows={2} value={reason} onChange={(e) => setReason(e.target.value)} autoFocus required />
      </label>
      {extra}
      {error && <p className="error">{error}</p>}
      <div className="critique__actions">
        <button type="submit" className={danger ? 'is-danger' : undefined} disabled={busy || !reason.trim()}>{confirm}</button>
        <button type="button" onClick={() => setOpen(false)}>Cancel</button>
      </div>
    </form>
  )
}

export const REASON_LABEL: Record<string, string> = {
  abuse: 'abusive',
  harassment: 'harassment',
  hate: 'hateful',
  sexual_content: 'sexual content',
  self_harm: 'self-harm',
  impersonation: 'impersonation',
  illegal: 'illegal',
  spam: 'spam',
  spoiler: 'spoiler',
  off_topic: 'off topic',
  other: 'other',
}

export const SOURCE_LABEL: Record<string, string> = {
  report: 'reports',
  word_filter: 'word filter',
  external: 'external platform',
  threshold: 'threshold',
}
