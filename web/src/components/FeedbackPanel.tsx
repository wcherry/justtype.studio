import { useCallback, useEffect, useState } from 'react'
import { api, downloadFeedback } from '../api/client'
import { REACTIONS } from '../editor/reactionGutter'
import type {
  AnswerView, BlockReactions, PublicationCard, QuestionKind, QuestionScope, ReportView,
  ReviewQuestion, VersionContent,
} from '../api/types'

interface Props {
  publication: PublicationCard
}

/**
 * What readers said without writing a comment.
 *
 * Reactions are the part authors cannot get any other way: nobody writes "I was
 * skimming here", but they will admit it in one tap, and forty taps in the same
 * chapter is a fact about the chapter.
 */
export function FeedbackPanel({ publication }: Props) {
  const [reactions, setReactions] = useState<BlockReactions[]>([])
  const [questions, setQuestions] = useState<ReviewQuestion[]>([])
  const [answers, setAnswers] = useState<AnswerView[]>([])
  const [chapters, setChapters] = useState<VersionContent['chapters']>([])
  const [reports, setReports] = useState<ReportView[]>([])
  const [loaded, setLoaded] = useState(false)
  const [prompt, setPrompt] = useState('')
  const [kind, setKind] = useState<QuestionKind>('rating')
  const [scope, setScope] = useState<QuestionScope>('chapter')
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(() => {
    void Promise.all([
      api.feedback.reactions(publication.id).then(setReactions).catch(() => setReactions([])),
      api.feedback.questions(publication.id).then(setQuestions).catch(() => setQuestions([])),
      api.feedback.answers(publication.id).then(setAnswers).catch(() => setAnswers([])),
      api.publications
        .content(publication.id)
        .then((c) => setChapters(c.chapters))
        .catch(() => setChapters([])),
      api.moderation.reports(publication.id).then(setReports).catch(() => setReports([])),
    ]).finally(() => setLoaded(true))
  }, [publication.id])

  useEffect(load, [load])

  const ask = async () => {
    setError(null)
    try {
      await api.feedback.ask(publication.id, { prompt: prompt.trim(), kind, scope })
      setPrompt('')
      load()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not ask that')
    }
  }

  const chapterTitle = (chapterId: string) =>
    chapters.find((c) => c.chapterId === chapterId)?.title ?? 'the whole thing'

  return (
    <div className="feedback-panel">
      {error && <p className="error">{error}</p>}

      {reports.length > 0 && (
        <section className="reports">
          <h4>Flagged by a reader</h4>
          <p className="field__hint">
            There is no moderation queue here — you are it. Nothing has been
            hidden; these are notes somebody else thought you should look at.
          </p>
          <ul>
            {reports.map((report) => (
              <li key={report.id}>
                <div className="reports__meta">
                  <span>
                    {report.reporter.displayName} flagged {report.commentAuthor.displayName}
                  </span>
                  <span className="badge badge--report">{report.reason.replace('_', ' ')}</span>
                </div>
                <p>{report.commentBody}</p>
                {report.detail && <p className="field__hint">{report.detail}</p>}
                <div className="reports__actions">
                  <button
                    type="button"
                    className="linklike"
                    onClick={async () => {
                      await api.comments.remove(report.commentId)
                      load()
                    }}
                  >
                    Delete the note
                  </button>
                  <button
                    type="button"
                    className="linklike"
                    title="They will not be able to open your work"
                    onClick={async () => {
                      await api.moderation.block(report.commentAuthor.id)
                      load()
                    }}
                  >
                    Block {report.commentAuthor.displayName}
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section>
        <h4>Where readers reacted</h4>
        {loaded && reactions.length === 0 && <p className="empty">No reactions yet.</p>}
        <ul className="heat">
          {reactions.map((block) => (
            <li key={`${block.sceneId}:${block.blockIndex}`}>
              <div className="heat__counts">
                {block.counts.map((count) => {
                  const face = REACTIONS.find((r) => r.kind === count.kind)
                  return (
                    <span key={count.kind} className={`heat__count heat__count--${count.kind}`}>
                      {face?.glyph} {count.count} {face?.noun}
                    </span>
                  )
                })}
              </div>
              <p className="heat__quote">{block.quote || '(that paragraph is gone)'}</p>
            </li>
          ))}
        </ul>
      </section>

      <section>
        <h4>Your questions</h4>
        <p className="field__hint">
          A chapter question is asked again at the end of every chapter, which is
          what turns “does the middle sag” into something with an answer.
        </p>

        <ul className="questions__list">
          {questions.map((question) => {
            const given = answers.filter((a) => a.questionId === question.id)
            return (
              <li key={question.id}>
                <div className="questions__row">
                  <span>{question.prompt}</span>
                  <button
                    type="button"
                    title="Stop asking this"
                    onClick={async () => {
                      await api.feedback.removeQuestion(question.id)
                      load()
                    }}
                  >
                    ×
                  </button>
                </div>
                <span className="field__hint">
                  {question.kind === 'rating' ? 'Rated 1–5' : 'Answered in words'} ·{' '}
                  {question.scope === 'chapter' ? 'per chapter' : 'once'} · {given.length} answer
                  {given.length === 1 ? '' : 's'}
                </span>
                <Answers question={question} answers={given} chapterTitle={chapterTitle} />
              </li>
            )
          })}
          {loaded && questions.length === 0 && (
            <li className="empty">You have not asked anything yet.</li>
          )}
        </ul>

        <form
          className="questions__new"
          onSubmit={(event) => {
            event.preventDefault()
            if (prompt.trim()) void ask()
          }}
        >
          <input
            value={prompt}
            placeholder="Did this chapter drag?"
            onChange={(e) => setPrompt(e.target.value)}
          />
          <select value={kind} onChange={(e) => setKind(e.target.value as QuestionKind)}>
            <option value="rating">1–5</option>
            <option value="text">In words</option>
          </select>
          <select value={scope} onChange={(e) => setScope(e.target.value as QuestionScope)}>
            <option value="chapter">Each chapter</option>
            <option value="work">Once</option>
          </select>
          <button type="submit" disabled={!prompt.trim()}>
            Ask
          </button>
        </form>
      </section>

      <section>
        <h4>Take it away with you</h4>
        <p className="field__hint">
          Every note on this publication as one document — the book's chapters,
          the passages people quoted, and what they said underneath.
        </p>
        <div className="feedback-panel__export">
          {(['txt', 'docx', 'pdf'] as const).map((format) => (
            <button
              key={format}
              type="button"
              onClick={async () => {
                setError(null)
                try {
                  await downloadFeedback(publication.id, format)
                } catch (e) {
                  setError(e instanceof Error ? e.message : 'Could not build that file')
                }
              }}
            >
              {format.toUpperCase()}
            </button>
          ))}
        </div>
      </section>
    </div>
  )
}

/** Ratings average; words are quoted. Neither is useful in the other's shape. */
function Answers({
  question,
  answers,
  chapterTitle,
}: {
  question: ReviewQuestion
  answers: AnswerView[]
  chapterTitle: (chapterId: string) => string
}) {
  if (answers.length === 0) return null

  if (question.kind === 'text') {
    return (
      <ul className="answers">
        {answers.map((answer) => (
          <li key={answer.id}>
            <span className="answers__who">{answer.reader.displayName}</span>
            {question.scope === 'chapter' && (
              <span className="field__hint">on {chapterTitle(answer.chapterId)}</span>
            )}
            <p>{answer.body}</p>
          </li>
        ))}
      </ul>
    )
  }

  const groups = new Map<string, number[]>()
  for (const answer of answers) {
    if (answer.rating == null) continue
    groups.set(answer.chapterId, [...(groups.get(answer.chapterId) ?? []), answer.rating])
  }

  return (
    <ul className="answers answers--ratings">
      {[...groups.entries()].map(([chapterId, ratings]) => {
        const mean = ratings.reduce((a, b) => a + b, 0) / ratings.length
        return (
          <li key={chapterId || 'work'}>
            <span className="answers__who">{chapterId ? chapterTitle(chapterId) : 'Overall'}</span>
            <span className="answers__bar">
              <span style={{ width: `${(mean / 5) * 100}%` }} />
            </span>
            <span className="field__hint">
              {mean.toFixed(1)} from {ratings.length}
            </span>
          </li>
        )
      })}
    </ul>
  )
}
