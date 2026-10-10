import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { ApiError, critiques } from '../api/client'
import type { CritiqueView, ReportReason } from '../api/types'

/** How far replies indent: six levels, three on a phone. Deeper ones carry
 *  on at that depth, saying who they answer. */
const MAX_INDENT = 6
const PHONE_INDENT = 3
const PHONE = '(max-width: 600px)'

function useIndentCap(): number {
  const query = typeof window !== 'undefined' && window.matchMedia ? window.matchMedia(PHONE) : null
  const [phone, setPhone] = useState(() => !!query?.matches)
  useEffect(() => {
    if (!query) return
    const change = () => setPhone(query.matches)
    query.addEventListener('change', change)
    return () => query.removeEventListener('change', change)
  }, [query])
  return phone ? PHONE_INDENT : MAX_INDENT
}

const REASONS: { value: ReportReason; label: string }[] = [
  { value: 'abuse', label: 'Abusive' },
  { value: 'spam', label: 'Spam' },
  { value: 'off_topic', label: 'Not about the writing' },
  { value: 'other', label: 'Something else' },
]

interface Node {
  critique: CritiqueView
  depth: number
  /** Who it answers, when that is not plain from its indent. */
  replyingTo: string | null
}

/** The tree, flattened in reading order with each critique's depth. */
function arrange(list: CritiqueView[], maxIndent: number): Node[] {
  const children = new Map<string | null, CritiqueView[]>()
  for (const c of list) {
    const key = c.parentId ?? null
    children.set(key, [...(children.get(key) ?? []), c])
  }
  const byId = new Map(list.map((c) => [c.id, c]))
  const out: Node[] = []
  const walk = (parent: string | null, depth: number) => {
    for (const c of children.get(parent) ?? []) {
      const shown = Math.min(depth, maxIndent)
      const parentName = c.parentId ? byId.get(c.parentId)?.author?.displayName ?? 'a removed critique' : null
      out.push({ critique: c, depth: shown, replyingTo: depth > maxIndent ? parentName : null })
      walk(c.id, depth + 1)
    }
  }
  walk(null, 0)
  return out
}

/**
 * Critiques on an answer, and replies to them to any depth. Every one carries
 * its writer's name. The writer of the answer can remove any of them; anyone
 * else can report one, which goes to the writer of the answer.
 */
export function CritiqueThread({
  entryId,
  open,
  isAuthor,
}: {
  entryId: string
  /** Submitted, so new critiques can be added. */
  open: boolean
  isAuthor: boolean
}) {
  const [list, setList] = useState<CritiqueView[]>([])
  const [error, setError] = useState<string | null>(null)
  const [draft, setDraft] = useState('')
  const [replyTo, setReplyTo] = useState<string | null>(null)
  const [reply, setReply] = useState('')
  const [editing, setEditing] = useState<string | null>(null)
  const [edit, setEdit] = useState('')
  const [reporting, setReporting] = useState<string | null>(null)
  const [reason, setReason] = useState<ReportReason>('off_topic')
  const [notice, setNotice] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      setList(await critiques.list(entryId))
      setError(null)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not load the critiques')
    }
  }, [entryId])

  useEffect(() => {
    void load()
  }, [load])

  // Arriving from the inbox at #critique-…: bring that one into view.
  useEffect(() => {
    const id = window.location.hash.replace('#', '')
    if (id && list.length) document.getElementById(id)?.scrollIntoView({ block: 'center' })
  }, [list.length])

  const cap = useIndentCap()
  const nodes = useMemo(() => arrange(list, cap), [list, cap])

  const act = async (work: () => Promise<unknown>, after?: () => void) => {
    setBusy(true)
    setError(null)
    try {
      await work()
      after?.()
      await load()
    } catch (e) {
      setError(e instanceof ApiError || e instanceof Error ? e.message : 'Could not do that')
    } finally {
      setBusy(false)
    }
  }

  const live = list.filter((c) => !c.removed).length

  return (
    <section className="critiques" aria-labelledby="critiques-heading">
      <h2 id="critiques-heading">{live === 1 ? '1 critique' : `${live} critiques`}</h2>
      {!open && isAuthor && list.length > 0 && (
        <p className="field__hint">Withdrawn: only you, and each critic their own, can see these until you submit again.</p>
      )}
      {error && <p className="error">{error}</p>}
      {notice && <p className="field__hint">{notice}</p>}
      <ol className="critiques__list">
        {nodes.map(({ critique: c, depth, replyingTo }) => (
          <li
            key={c.id}
            id={`critique-${c.id}`}
            className={`critique${c.removed ? ' is-removed' : ''}`}
            style={{ '--depth': depth } as React.CSSProperties}
          >
            {c.removed ? (
              <p className="comment__removed">Removed</p>
            ) : (
              <>
                <p className="comment__meta">
                  {c.author && <Link to={`/people/${c.author.id}`}>{c.author.displayName}</Link>}
                  {replyingTo && <span className="field__hint"> replying to {replyingTo}</span>}
                  <time className="field__hint" dateTime={c.createdAt}>
                    {' · '}
                    {new Date(c.createdAt).toLocaleString()}
                  </time>
                  {c.editedAt && <span className="field__hint" title={new Date(c.editedAt).toLocaleString()}> · edited</span>}
                </p>
                {editing === c.id ? (
                  <form
                    className="critique__form"
                    onSubmit={(e) => {
                      e.preventDefault()
                      void act(() => critiques.update(c.id, edit), () => setEditing(null))
                    }}
                  >
                    <textarea aria-label="Your critique" rows={3} value={edit} onChange={(e) => setEdit(e.target.value)} />
                    <div className="critique__actions">
                      <button type="submit" disabled={busy || !edit.trim()}>Save</button>
                      <button type="button" onClick={() => setEditing(null)}>Cancel</button>
                    </div>
                  </form>
                ) : (
                  <p className="critique__body">{c.body}</p>
                )}
                <div className="comment__actions">
                  {open && (
                    <button type="button" onClick={() => { setReplyTo(c.id); setReply('') }}>Reply</button>
                  )}
                  {c.mine && editing !== c.id && (
                    <button type="button" onClick={() => { setEditing(c.id); setEdit(c.body) }}>Edit</button>
                  )}
                  {c.canRemove && (
                    <button
                      type="button"
                      onClick={() => {
                        if (window.confirm(c.mine ? 'Delete your critique?' : `Remove ${c.author?.displayName ?? 'this'}'s critique from your answer?`)) {
                          void act(() => critiques.remove(c.id))
                        }
                      }}
                    >
                      {c.mine ? 'Delete' : 'Remove'}
                    </button>
                  )}
                  {!c.mine && !isAuthor && (
                    <button type="button" onClick={() => setReporting(reporting === c.id ? null : c.id)}>Report</button>
                  )}
                </div>
                {reporting === c.id && (
                  <div className="comment__report">
                    <select aria-label="Why" value={reason} onChange={(e) => setReason(e.target.value as ReportReason)}>
                      {REASONS.map((r) => <option key={r.value} value={r.value}>{r.label}</option>)}
                    </select>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() =>
                        void act(() => critiques.report(c.id, reason), () => {
                          setReporting(null)
                          setNotice('Reported. The writer of this answer has been told.')
                        })
                      }
                    >
                      Send
                    </button>
                  </div>
                )}
                {replyTo === c.id && (
                  <form
                    className="critique__form"
                    onSubmit={(e) => {
                      e.preventDefault()
                      void act(() => critiques.create(entryId, reply, c.id), () => setReplyTo(null))
                    }}
                  >
                    <textarea
                      aria-label={`Reply to ${c.author?.displayName ?? 'this critique'}`}
                      rows={3}
                      autoFocus
                      value={reply}
                      onChange={(e) => setReply(e.target.value)}
                    />
                    <div className="critique__actions">
                      <button type="submit" disabled={busy || !reply.trim()}>Reply</button>
                      <button type="button" onClick={() => setReplyTo(null)}>Cancel</button>
                    </div>
                  </form>
                )}
              </>
            )}
          </li>
        ))}
      </ol>
      {open && !isAuthor && (
        <form
          className="critique__form critique__new"
          onSubmit={(e) => {
            e.preventDefault()
            void act(() => critiques.create(entryId, draft), () => setDraft(''))
          }}
        >
          <label>
            Your critique
            <textarea rows={4} value={draft} onChange={(e) => setDraft(e.target.value)} />
          </label>
          <p className="field__hint">Your name is shown with it. Everyone in Daily Prompts can read it.</p>
          <button type="submit" disabled={busy || !draft.trim()}>Post</button>
        </form>
      )}
    </section>
  )
}
