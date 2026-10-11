import { useEffect, useState } from 'react'
import { ReportButton } from './ReportButton'
import { api } from '../api/client'
import type { AnswerView, ReviewQuestion } from '../api/types'

interface Props {
  question: ReviewQuestion
  /** Empty for a work-scoped question. */
  chapterId: string
  existing: AnswerView | undefined
  onAnswered: (answer: AnswerView) => void
}

/**
 * One question the author asked, answered in place.
 *
 * A chapter question is asked where the chapter ends rather than in a form at
 * the end of everything: the answer to "did this chapter drag" is worth most
 * the moment the reader has finished it and least an hour later.
 */
export function QuestionCard({ question, chapterId, existing, onAnswered }: Props) {
  const [text, setText] = useState(existing?.body ?? '')
  const [saved, setSaved] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => setText(existing?.body ?? ''), [existing?.body])

  const send = async (input: { rating?: number; body?: string }) => {
    setBusy(true)
    setError(null)
    try {
      const answer = await api.feedback.answer(question.id, { chapterId, ...input })
      onAnswered(answer)
      setSaved(true)
      window.setTimeout(() => setSaved(false), 1600)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not send that')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="question">
      <p className="question__prompt">
        {question.prompt} <ReportButton subjectType="question" subjectId={question.id} />
      </p>

      {question.kind === 'rating' ? (
        <div className="question__scale">
          {[1, 2, 3, 4, 5].map((n) => (
            <button
              key={n}
              type="button"
              disabled={busy}
              className={existing?.rating === n ? 'is-picked' : undefined}
              onClick={() => send({ rating: n })}
            >
              {n}
            </button>
          ))}
          <span className="field__hint">1 dragged · 5 flew by</span>
        </div>
      ) : (
        <form
          onSubmit={(event) => {
            event.preventDefault()
            if (text.trim()) void send({ body: text.trim() })
          }}
        >
          <textarea
            rows={2}
            value={text}
            placeholder="In your own words"
            onChange={(e) => setText(e.target.value)}
          />
          <button type="submit" disabled={busy || !text.trim()}>
            {existing ? 'Change my answer' : 'Send'}
          </button>
        </form>
      )}

      {error && <p className="error">{error}</p>}
      {saved && !error && <p className="field__hint">Thank you — sent to the author.</p>}
    </div>
  )
}
