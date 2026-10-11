import { useEffect, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import { staff } from '../../api/client'
import type { QueueItem } from '../../api/types'
import { Pill, REASON_LABEL, ReasonButton, SOURCE_LABEL, day, errorText, when } from './common'

const TABS: [string, string][] = [
  ['content', 'Content'],
  ['accounts', 'Accounts'],
  ['held', 'Held'],
  ['done', 'Done'],
]

const SUBJECTS = ['comment', 'question', 'answer', 'entry', 'critique', 'profile', 'publication', 'account']

/** What the item was flagged by, in words: "3 reports · word filter: hold · external: remove". */
function flaggedBy(item: QueueItem): string {
  const parts = item.sourcesList.map((s) => {
    if (s === 'report') return `${item.reportCount} report${item.reportCount === 1 ? '' : 's'}`
    let extra = ''
    try {
      const labels = JSON.parse(item.labels ?? '{}') as { action?: string; labels?: string[]; recommended?: string }
      if (s === 'word_filter' && labels.action) extra = `: ${labels.action}`
      if (s === 'external') extra = `: ${labels.recommended ?? 'flag'}${labels.labels?.length ? `, ${labels.labels.join(', ')}` : ''}`
      if (s === 'threshold') extra = ''
    } catch {
      // Labels are for the eye; a bad one is just left out.
    }
    return `${SOURCE_LABEL[s] ?? s}${extra}`
  })
  return parts.join(' · ')
}

/** Flagged content and flagged accounts, worst first, with the actions beside them. */
export function AdminQueuePage() {
  const [params, setParams] = useSearchParams()
  const tab = params.get('tab') ?? 'content'
  const source = params.get('source') ?? ''
  const reason = params.get('reason') ?? ''
  const surface = params.get('surface') ?? ''
  const mine = params.get('mine') === 'true'
  const [items, setItems] = useState<QueueItem[]>([])
  const [total, setTotal] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())

  const load = () => {
    setError(null)
    staff
      .queue({ tab, source, reason, surface, mine: mine || undefined })
      .then((page) => {
        setItems(page.items)
        setTotal(page.total)
        setPicked(new Set())
      })
      .catch((e) => setError(errorText(e)))
  }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [tab, source, reason, surface, mine])

  const set = (key: string, value: string) => {
    const next = new URLSearchParams(params)
    if (value) next.set(key, value)
    else next.delete(key)
    setParams(next, { replace: true })
  }

  const decide = async (item: QueueItem, action: 'dismiss' | 'remove' | 'restore' | 'hold' | 'release', why: string) => {
    await staff.decide(item.id, action, why)
    setMessage(
      action === 'remove' ? 'Removed. Its author has been told.'
        : action === 'dismiss' ? 'Dismissed.'
        : action === 'hold' ? 'Held for review.'
        : action === 'release' ? 'Released.'
        : 'Restored.',
    )
    load()
  }

  // Bulk dismiss is for items that came from the same single source.
  const sameSource = (() => {
    const chosen = items.filter((i) => picked.has(i.id))
    if (chosen.length === 0) return null
    const first = chosen[0].sourcesList
    return first.length === 1 && chosen.every((i) => i.sourcesList.length === 1 && i.sourcesList[0] === first[0]) ? first[0] : null
  })()

  return (
    <main className="feed admin">
      <header className="feed__header"><h1>Moderation queue</h1></header>
      <div className="admin__tabs" role="tablist">
        {TABS.map(([id, label]) => (
          <button key={id} type="button" role="tab" aria-selected={tab === id} className={tab === id ? 'is-active' : undefined} onClick={() => set('tab', id === 'content' ? '' : id)}>
            {label}
          </button>
        ))}
      </div>
      <div className="admin__filters">
        <select aria-label="Source" value={source} onChange={(e) => set('source', e.target.value)}>
          <option value="">Any source</option>
          {Object.entries(SOURCE_LABEL).map(([v, l]) => <option key={v} value={v}>{l}</option>)}
        </select>
        <select aria-label="Reason" value={reason} onChange={(e) => set('reason', e.target.value)}>
          <option value="">Any reason</option>
          {Object.entries(REASON_LABEL).map(([v, l]) => <option key={v} value={v}>{l}</option>)}
        </select>
        <select aria-label="Surface" value={surface} onChange={(e) => set('surface', e.target.value)}>
          <option value="">Anywhere</option>
          {SUBJECTS.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
        <label className="auth__check"><input type="checkbox" checked={mine} onChange={(e) => set('mine', e.target.checked ? 'true' : '')} /><span>Assigned to me</span></label>
      </div>
      {message && <p className="banner" role="status">{message}</p>}
      {error && <p className="error">{error}</p>}
      <p className="field__hint">{total} item{total === 1 ? '' : 's'}</p>
      {sameSource && picked.size > 0 && (
        <ReasonButton
          label={`Dismiss ${picked.size} selected`}
          onDo={async (why) => {
            await staff.bulkDismiss([...picked], sameSource, why)
            setMessage(`Dismissed ${picked.size}.`)
            load()
          }}
        />
      )}
      {items.length === 0 && <p className="empty">Nothing here.</p>}
      <ul className="admin__list admin__queue-list">
        {items.map((item) => (
          <Item
            key={item.id}
            item={item}
            picked={picked.has(item.id)}
            onPick={(on) => setPicked((p) => { const n = new Set(p); if (on) n.add(item.id); else n.delete(item.id); return n })}
            decide={(action, why) => decide(item, action, why)}
            onChanged={load}
            focused={params.get('focus') === item.id}
          />
        ))}
      </ul>
    </main>
  )
}

function Item({ item, picked, onPick, decide, onChanged, focused }: {
  item: QueueItem
  picked: boolean
  onPick: (on: boolean) => void
  decide: (action: 'dismiss' | 'remove' | 'restore' | 'hold' | 'release', reason: string) => Promise<void>
  onChanged: () => void
  focused: boolean
}) {
  const [open, setOpen] = useState(focused)
  const [claimNote, setClaimNote] = useState<string | null>(null)
  const isAccount = item.subjectType === 'account'
  const live = item.state === 'open' || item.state === 'held'
  const content = ['comment', 'question', 'answer', 'entry', 'critique'].includes(item.subjectType)

  // Opening an item claims it for ten minutes, so two staff don't work on one.
  const toggle = async () => {
    if (!open && live) {
      try {
        await staff.claim(item.id)
        setClaimNote(null)
      } catch (e) {
        setClaimNote(errorText(e))
      }
    }
    setOpen((o) => !o)
  }

  return (
    <li className={`admin__queue-item${item.severity >= 90 ? ' is-urgent' : ''}`}>
      <div className="admin__queue-head">
        {live && <input type="checkbox" aria-label="Select" checked={picked} onChange={(e) => onPick(e.target.checked)} />}
        <Pill tone={item.severity >= 90 ? 'bad' : item.severity >= 50 ? 'warn' : 'plain'}>{item.subjectType}</Pill>
        {item.topReason && <Pill>{REASON_LABEL[item.topReason] ?? item.topReason}</Pill>}
        <strong>{item.author.displayName}</strong>
        {item.authorEmail && <span className="field__hint"> {item.authorEmail}</span>}
        <span className="field__hint"> · {flaggedBy(item)} · {when(item.lastFlaggedAt)}</span>
        {item.state !== 'open' && <Pill tone={item.state === 'held' ? 'warn' : 'plain'}>{item.state}{item.decision ? ` · ${item.decision.replace(/_/g, ' ')}` : ''}</Pill>}
        {item.subjectState !== 'visible' && <Pill tone="warn">{item.subjectState}</Pill>}
        {item.claimedByName && <Pill>{item.claimedByName} has it</Pill>}
        <button type="button" className="linklike" onClick={() => void toggle()}>{open ? 'Close' : 'Open'}</button>
      </div>
      {item.text && <blockquote className="admin__quote">{item.text.length > 600 && !open ? `${item.text.slice(0, 600)}…` : item.text}</blockquote>}
      {open && (
        <div className="admin__queue-body">
          {claimNote && <p className="error">{claimNote}</p>}
          {item.context && <p className="field__hint">Where: {item.context}</p>}
          {item.reports.length > 0 && (
            <>
              <h4>Reporters</h4>
              <ul className="admin__list">
                {item.reports.map((r, i) => (
                  <li key={i}>
                    <Link to={`/admin/accounts/${r.reporter.id}`}>{r.reporter.displayName}</Link> · {REASON_LABEL[r.reason] ?? r.reason} · {day(r.createdAt)}
                    {r.detail && <> — “{r.detail}”</>}
                  </li>
                ))}
              </ul>
            </>
          )}
          <p>
            <Link to={`/admin/accounts/${item.subjectAuthorId}`}>Open the account</Link>
          </p>
          <div className="admin__actions">
            {live && <ReasonButton label="Dismiss" onDo={(why) => decide('dismiss', why)} />}
            {live && content && item.state === 'open' && <ReasonButton label="Hold" onDo={(why) => decide('hold', why)} />}
            {live && content && item.subjectState !== 'removed' && (
              <ReasonButton label="Remove" prompt="Which part of the guidelines, and why? The author is told." danger onDo={(why) => decide('remove', why)} />
            )}
            {item.state === 'held' && content && <ReasonButton label="Release" onDo={(why) => decide('release', why)} />}
            {content && item.subjectState === 'removed' && <ReasonButton label="Restore" onDo={(why) => decide('restore', why)} />}
            {!isAccount || live ? (
              <Link className="button" to={`/admin/accounts/${item.subjectAuthorId}?restrict=1&item=${item.id}`} onClick={onChanged}>
                Restrict the author…
              </Link>
            ) : null}
          </div>
        </div>
      )}
    </li>
  )
}
