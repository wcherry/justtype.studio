import { useState } from 'react'
import { ApiError, standing } from '../api/client'
import type { ReportReason } from '../api/types'
import { useStanding } from '../state/standing'

/** What a reader can say is wrong. The first four go to staff as well as to the author. */
export const REPORT_REASONS: { value: ReportReason; label: string }[] = [
  { value: 'harassment', label: 'Harassment or bullying' },
  { value: 'hate', label: 'Hateful towards a group' },
  { value: 'sexual_content', label: 'Sexual content' },
  { value: 'self_harm', label: 'Talk of self-harm' },
  { value: 'impersonation', label: 'Pretending to be someone' },
  { value: 'illegal', label: 'Illegal' },
  { value: 'abuse', label: 'Abusive' },
  { value: 'spam', label: 'Spam' },
  { value: 'other', label: 'Something else' },
]

type Subject = 'entry' | 'question' | 'answer' | 'profile' | 'publication'

/**
 * A **Report** control for the things that had none: a Daily Prompts answer,
 * a reader's question or answer, a profile, a publication. Goes to site staff.
 */
export function ReportButton({ subjectType, subjectId, label = 'Report' }: {
  subjectType: Subject
  subjectId: string
  label?: string
}) {
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState<ReportReason>('harassment')
  const [detail, setDetail] = useState('')
  const [done, setDone] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const { blocked } = useStanding()
  const why = blocked('report')

  if (done) return <span className="field__hint report__done">{done}</span>
  return (
    <span className="report">
      <button type="button" className="linklike" disabled={!!why} title={why ?? undefined} onClick={() => setOpen((o) => !o)}>
        {label}
      </button>
      {open && (
        <form
          className="comment__report"
          onSubmit={async (event) => {
            event.preventDefault()
            setError(null)
            try {
              const r = await standing.report({ subjectType, subjectId, reason, detail: detail || undefined })
              setDone(r.support ?? 'Reported. Thank you — the site’s staff will look at it.')
              setOpen(false)
            } catch (e) {
              setError(e instanceof ApiError ? e.message : 'Could not send that')
            }
          }}
        >
          <select aria-label="Why" value={reason} onChange={(e) => setReason(e.target.value as ReportReason)}>
            {REPORT_REASONS.map((r) => <option key={r.value} value={r.value}>{r.label}</option>)}
          </select>
          <input
            aria-label="Anything we should know (optional)"
            placeholder="Anything we should know? (optional)"
            maxLength={1000}
            value={detail}
            onChange={(e) => setDetail(e.target.value)}
          />
          <button type="submit">Send</button>
          {error && <span className="error">{error}</span>}
        </form>
      )}
    </span>
  )
}
