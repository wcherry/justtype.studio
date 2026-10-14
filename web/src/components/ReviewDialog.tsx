import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import type { AuthorCard, PublicationScope, ReviewAudience, ReviewView } from '../api/types'

/** What is being put up for review, as the outline names it. */
export interface ReviewTarget {
  scope: PublicationScope
  /** The part, chapter or scene; absent for the whole book. */
  scopeRefId?: string
  /** "“The Door”", or the book's title. */
  title: string
}

const KIND: Record<PublicationScope, string> = {
  work: 'the whole book',
  part: 'this part',
  chapter: 'this chapter',
  scene: 'this scene',
}

/** A date input's value from a stored instant, in the writer's own time zone. */
function dayOf(iso: string | null | undefined): string {
  if (!iso) return ''
  const at = new Date(iso)
  if (Number.isNaN(at.getTime())) return ''
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`
}

/** The end of that day where the writer is: a review "closes on the 3rd" all through the 3rd. */
function endOf(day: string): string {
  const [y, m, d] = day.split('-').map(Number)
  return new Date(y, m - 1, d, 23, 59, 59).toISOString()
}

/**
 * Marks a scene, chapter, part or the whole book for review, or changes a
 * review already up: who it is for, what feedback is wanted, when it closes.
 *
 * People are picked by name from the site, or by address for someone who has
 * no account yet. Readers are shown a snapshot: the latest one if nothing has
 * changed since it, a new one otherwise — the server decides which.
 */
export function ReviewDialog({
  workId, target, review, onSaved, onClose,
}: {
  workId: string
  target: ReviewTarget
  /** The review being changed; absent when marking something new. */
  review?: ReviewView
  onSaved: (review: ReviewView) => void
  onClose: () => void
}) {
  const ref = useRef<HTMLDialogElement>(null)
  const [audience, setAudience] = useState<ReviewAudience>(
    review ? (review.visibility === 'invited' ? 'selected' : 'everyone') : 'everyone',
  )
  const [picked, setPicked] = useState<AuthorCard[]>(
    () => review?.people.flatMap((p) => (p.user ? [p.user] : [])) ?? [],
  )
  const [emails, setEmails] = useState<string[]>(
    () => review?.people.flatMap((p) => (p.email ? [p.email] : [])) ?? [],
  )
  const [query, setQuery] = useState('')
  const [found, setFound] = useState<AuthorCard[]>([])
  const [prompt, setPrompt] = useState(review?.prompt ?? '')
  const [closes, setCloses] = useState(dayOf(review?.closesAt))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  // Names as they are typed; an address is added as it stands.
  useEffect(() => {
    const q = query.trim()
    if (q.length < 2 || q.includes('@')) {
      setFound([])
      return
    }
    let live = true
    const timer = setTimeout(() => {
      api.people(q)
        .then((people) => live && setFound(people.filter((p) => !picked.some((x) => x.id === p.id))))
        .catch(() => live && setFound([]))
    }, 200)
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [query, picked])

  const looksLikeEmail = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(query.trim())
  const addEmail = () => {
    const email = query.trim().toLowerCase()
    if (!looksLikeEmail) return
    if (!emails.includes(email)) setEmails([...emails, email])
    setQuery('')
  }
  const pick = (person: AuthorCard) => {
    setPicked([...picked, person])
    setQuery('')
    setFound([])
  }

  const nobody = audience === 'selected' && picked.length === 0 && emails.length === 0

  const save = async (e: React.FormEvent) => {
    e.preventDefault()
    if (nobody) return
    setBusy(true)
    setError(null)
    const people = audience === 'selected'
      ? { userIds: picked.map((p) => p.id), emails }
      : {}
    try {
      const saved = review
        ? await api.reviews.update(review.id, {
            audience,
            ...(audience === 'selected' ? people : {}),
            prompt: prompt.trim(),
            closesAt: closes ? endOf(closes) : '',
          })
        : await api.reviews.create(workId, {
            scope: target.scope,
            scopeRefId: target.scopeRefId,
            audience,
            ...people,
            prompt: prompt.trim() || undefined,
            closesAt: closes ? endOf(closes) : undefined,
          })
      onSaved(saved)
      ref.current?.close()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'That did not work')
    } finally {
      setBusy(false)
    }
  }

  return (
    <dialog
      ref={ref}
      className="dialog review-dialog"
      aria-labelledby="review-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <form onSubmit={save}>
        <h2 id="review-title">{review ? 'Change the review of' : 'Mark for review:'} {target.title}</h2>
        {!review && (
          <p className="field__hint">
            Readers see {KIND[target.scope]} as it stands now. If nothing has changed since your last snapshot of it,
            that snapshot is used; otherwise a new one is saved. You can bring the review up to date later.
          </p>
        )}

        <fieldset className="review-dialog__audience">
          <legend>Who can review it</legend>
          <label>
            <input type="radio" name="audience" checked={audience === 'everyone'} onChange={() => setAudience('everyone')} />
            Public
            <span className="field__hint">Anyone signed in to Just Type Studio. Listed in the review feed.</span>
          </label>
          <label>
            <input type="radio" name="audience" checked={audience === 'selected'} onChange={() => setAudience('selected')} />
            Only people I choose
          </label>
        </fieldset>

        {audience === 'selected' && (
          <div className="review-dialog__people">
            <ul className="review-dialog__chips" aria-label="Chosen reviewers">
              {picked.map((p) => (
                <li key={p.id} className="chip">
                  {p.displayName}
                  <button type="button" aria-label={`Remove ${p.displayName}`} onClick={() => setPicked(picked.filter((x) => x.id !== p.id))}>✕</button>
                </li>
              ))}
              {emails.map((email) => (
                <li key={email} className="chip chip--email">
                  {email}
                  <button type="button" aria-label={`Remove ${email}`} onClick={() => setEmails(emails.filter((x) => x !== email))}>✕</button>
                </li>
              ))}
            </ul>
            <div className="review-dialog__search">
              <input
                value={query}
                placeholder="A name, or an email address"
                aria-label="Add a reviewer by name or email"
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    if (looksLikeEmail) addEmail()
                    else if (found[0]) pick(found[0])
                  }
                }}
              />
              {looksLikeEmail && <button type="button" onClick={addEmail}>Invite {query.trim()}</button>}
              {found.length > 0 && (
                <ul className="review-dialog__found" role="listbox" aria-label="People found">
                  {found.map((p) => (
                    <li key={p.id}>
                      <button type="button" role="option" aria-selected={false} onClick={() => pick(p)}>{p.displayName}</button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <p className="field__hint">
              People with an account get a note in their inbox. An address gets an email; they read it once they sign
              in with that address.
            </p>
          </div>
        )}

        <label>
          What do you want feedback on?
          <textarea rows={3} value={prompt} placeholder="Does the opening earn the next page?" onChange={(e) => setPrompt(e.target.value)} />
        </label>
        <label>
          Closes on
          <input type="date" value={closes} onChange={(e) => setCloses(e.target.value)} />
          <span className="field__hint">Leave it empty to keep it open until you revoke it.</span>
        </label>

        {error && <p className="error">{error}</p>}
        {nobody && <p className="field__hint">Choose at least one person, or make it public.</p>}
        <div className="dialog__actions">
          <button type="submit" disabled={busy || nobody}>
            {busy ? 'Saving…' : review ? 'Save' : 'Mark for review'}
          </button>
          <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
        </div>
      </form>
    </dialog>
  )
}
