import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { ApiError, critiques, prompts } from '../api/client'
import type { CritiqueReportView, PromptEntryView } from '../api/types'
import { EntryText } from '../prompts/EntryText'
import { CritiqueThread } from '../prompts/CritiqueThread'
import { ToWorkDialog } from '../prompts/ToWorkDialog'
import { dayName } from '../prompts/days'

const REASON_LABEL: Record<string, string> = {
  abuse: 'abusive',
  spam: 'spam',
  spoiler: 'a spoiler',
  off_topic: 'not about the writing',
  other: 'something else',
}

/** One answer, whole, in the reader's typography, with its critiques below. */
export function PromptEntryPage() {
  const { entryId = '' } = useParams()
  const [entry, setEntry] = useState<PromptEntryView | null>(null)
  const [reports, setReports] = useState<CritiqueReportView[]>([])
  const [error, setError] = useState<string | null>(null)
  const [converting, setConverting] = useState(false)

  useEffect(() => {
    setEntry(null)
    prompts
      .get(entryId)
      .then((e) => {
        setEntry(e)
        if (e.mine) critiques.reports(entryId).then(setReports).catch(() => undefined)
      })
      .catch((e) =>
        setError(
          e instanceof ApiError && e.status === 403
            ? 'Join Daily Prompts to read the answers.'
            : e instanceof ApiError && e.status === 404
              ? 'That answer is not here: it may have been withdrawn or deleted.'
              : e.message,
        ),
      )
  }, [entryId])

  return (
    <main className="feed prompts prompt-entry">
      <p className="prompts__back">
        <Link to={entry ? `/prompts/${entry.day}` : '/prompts'}>← {entry ? dayName(entry.day) : 'Daily Prompts'}</Link>
      </p>
      {error && <p className="error">{error}</p>}
      {entry && (
        <>
          <blockquote className="prompt-write__prompt">{entry.prompt}</blockquote>
          <p className="feed__meta">
            <Link to={`/people/${entry.author.id}`}>{entry.author.displayName}</Link>
            {' · '}
            {entry.wordCount.toLocaleString()} words
            {entry.submittedAt && <> · {new Date(entry.submittedAt).toLocaleString()}</>}
            {entry.late && <span className="badge badge--late">late</span>}
            {entry.editedAt && <span className="badge" title={new Date(entry.editedAt).toLocaleString()}>edited</span>}
            {entry.mine && entry.state !== 'submitted' && <span className="badge">{entry.state}</span>}
          </p>
          {entry.mine && (
            <p className="prompt-entry__mine">
              <Link to={`/prompts/${entry.day}/write`}>Edit</Link>
              <button type="button" className="linklike" onClick={() => setConverting(true)}>Turn into a project</button>
              {entry.convertedWork && (
                <span className="field__hint">
                  Turned into <Link to={`/works/${entry.convertedWork.id}`}>{entry.convertedWork.title}</Link>
                </span>
              )}
            </p>
          )}
          <EntryText content={entry.content as never} />
          {entry.mine && reports.length > 0 && (
            <section className="prompt-entry__reports">
              <h2>Reported to you</h2>
              <ul>
                {reports.map((r) => (
                  <li key={r.id}>
                    {r.reporter.displayName} reported {r.critique.author?.displayName ?? 'a'}’s critique as {REASON_LABEL[r.reason] ?? r.reason}:{' '}
                    <q>{r.critique.body}</q>{' '}
                    <a href={`#critique-${r.critique.id}`}>Go to it</a>
                  </li>
                ))}
              </ul>
              <p className="field__hint">Remove it from the thread below if you agree.</p>
            </section>
          )}
          <CritiqueThread entryId={entry.id} open={entry.state === 'submitted'} isAuthor={entry.mine} />
        </>
      )}
      {converting && entry && <ToWorkDialog entryId={entry.id} onClose={() => setConverting(false)} />}
    </main>
  )
}
