import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { NoteFate, NoteStatus, PublicationCard, SinceResponse } from '../api/types'

interface Props {
  publication: PublicationCard
}

const GROUPS: { status: NoteStatus; title: string; hint: string }[] = [
  {
    status: 'untouched',
    title: 'Still stands',
    hint: 'The paragraph reads exactly as it did when they read it.',
  },
  {
    status: 'edited',
    title: 'You have written this since',
    hint: 'Probably answered. Worth a read before you resolve it.',
  },
  {
    status: 'gone',
    title: 'On text that has gone',
    hint: 'The paragraph or the scene is no longer there.',
  },
  {
    status: 'unanchored',
    title: 'General notes',
    hint: 'Anchored to nothing in particular, so nothing under them changed.',
  },
]

/**
 * What has changed since reviewers saw this, note by note.
 *
 * A diff answers "what did I change". The question an author actually has in
 * front of a pile of feedback is which of these they have already dealt with,
 * and that is a different question: it is asked of each note, against the
 * paragraph it was anchored to.
 */
export function SincePanel({ publication }: Props) {
  const [since, setSince] = useState<SinceResponse | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const load = useCallback(() => {
    setError(null)
    api.publications
      .since(publication.id)
      .then(setSince)
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not compare those'))
  }, [publication.id])

  useEffect(load, [load])

  if (error) return <div className="since"><p className="error">{error}</p></div>
  if (!since) return <div className="since"><p className="empty">Comparing…</p></div>

  const { summary, counts } = since
  const answered = since.notes.filter((n) => n.status === 'edited' && n.state === 'open')

  return (
    <div className="since">
      <p className="since__summary">
        Since <strong>{since.from}</strong> went out, against <strong>{since.to.toLowerCase()}</strong>:{' '}
        {summary.scenesModified} scene{summary.scenesModified === 1 ? '' : 's'} rewritten,{' '}
        {summary.paragraphsChanged} paragraph{summary.paragraphsChanged === 1 ? '' : 's'} changed,{' '}
        {summary.wordsAdded.toLocaleString()} words added and{' '}
        {summary.wordsRemoved.toLocaleString()} cut.
      </p>

      {counts.edited > 0 && answered.length > 0 && (
        <button
          type="button"
          className="linklike since__resolve"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              await api.comments.bulkState(publication.id, {
                state: 'resolved',
                commentIds: answered.map((n) => n.commentId),
              })
              load()
            } finally {
              setBusy(false)
            }
          }}
        >
          Resolve the {answered.length} you have already written
        </button>
      )}

      {since.notes.length === 0 && <p className="empty">Nobody has said anything yet.</p>}

      {GROUPS.map(({ status, title, hint }) => {
        const notes = since.notes.filter((note) => note.status === status)
        if (notes.length === 0) return null
        return (
          <section key={status} className={`since__group since__group--${status}`}>
            <h4>
              {title} <span className="since__count">{notes.length}</span>
            </h4>
            <p className="field__hint">{hint}</p>
            <ul>
              {notes.map((note) => (
                <Note key={note.commentId} note={note} />
              ))}
            </ul>
          </section>
        )
      })}
    </div>
  )
}

function Note({ note }: { note: NoteFate }) {
  return (
    <li>
      <div className="since__meta">
        <strong>{note.author.displayName}</strong>
        <span className={`badge badge--${note.category}`}>{note.category}</span>
        {note.state === 'resolved' && <span className="badge badge--resolved">resolved</span>}
        {note.sceneTitle && <span className="field__hint">{note.sceneTitle}</span>}
      </div>
      <p className="since__body">{note.body}</p>

      {note.quote && <blockquote className="since__quote">{note.quote}</blockquote>}

      {/* The paragraph as it stands now, with what moved marked, so the author
          can see whether the note was answered without opening the scene. */}
      {note.changes && note.changes.length > 0 && (
        <p className="since__now">
          {note.changes.map((change, index) => (
            <span
              key={index}
              className={change.op === 'equal' ? undefined : `diff__${change.op}`}
            >
              {change.value}
            </span>
          ))}
        </p>
      )}
    </li>
  )
}
