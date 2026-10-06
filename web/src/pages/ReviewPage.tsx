import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router-dom'
import { api, shareKeys } from '../api/client'
import type { CommentFilters } from '../api/client'
import { parseAnchor } from '../api/types'
import type {
  AnswerView, BlockReactions, CommentView, PublicationCard, ReviewQuestion, VersionContent,
} from '../api/types'
import { CommentSidebar } from '../components/CommentSidebar'
import { QuestionCard } from '../components/QuestionCard'
import { ReaderScene } from '../components/ReaderScene'
import type { Selection } from '../components/ReaderScene'
import type { AnchorSpec } from '../editor/commentAnchors'
import type { ReactionKind } from '../editor/reactionGutter'
import { useReadingProgress } from '../reader/useReadingProgress'
import { useAuth } from '../state/auth'
import { useLive } from '../live'

export function ReviewPage() {
  const { publicationId = '' } = useParams()
  const [params, setParams] = useSearchParams()
  // A chapter at a time: a novel is eighty scenes, and the first page should
  // not wait for the last. In the URL, so a reload keeps the reader's place.
  const page = Math.max(0, Number(params.get('chapter') ?? 0) || 0)
  // A link to an unlisted draft carries its key once; it is kept for the tab
  // and taken out of the address bar, where it would be bookmarked and shared.
  const linkKey = params.get('key')
  if (linkKey) shareKeys.remember(linkKey)
  useEffect(() => {
    if (!linkKey) return
    setParams((current) => {
      const updated = new URLSearchParams(current)
      updated.delete('key')
      return updated
    }, { replace: true })
  }, [linkKey, setParams])
  const { user } = useAuth()
  const [card, setCard] = useState<PublicationCard | null>(null)
  const [content, setContent] = useState<VersionContent | null>(null)
  const [comments, setComments] = useState<CommentView[]>([])
  const [pending, setPending] = useState<Selection | null>(null)
  const [activeCommentId, setActiveCommentId] = useState<string | null>(null)
  const [reactions, setReactions] = useState<BlockReactions[]>([])
  const [questions, setQuestions] = useState<ReviewQuestion[]>([])
  const [answers, setAnswers] = useState<AnswerView[]>([])
  const [filters, setFilters] = useState<CommentFilters>({ sort: 'top' })
  const [notice, setNotice] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const loadComments = useCallback(() => {
    api.comments.list(publicationId, filters).then(setComments).catch(() => undefined)
  }, [publicationId, filters])

  // Notes arrive as they are written. The channel only says that something
  // changed; the list is fetched again, through the same filters and rules.
  const [readers, setReaders] = useState(0)
  useLive(`publication:${publicationId}`, (notice) => {
    if (notice.type === 'comments') loadComments()
    if (notice.type === 'presence' && typeof notice.readers === 'number') setReaders(notice.readers)
  })

  useEffect(() => {
    api.publications.get(publicationId)
      .then(setCard)
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not open that publication'))
    loadComments()
    void api.feedback.reactions(publicationId).then(setReactions).catch(() => setReactions([]))
    void api.feedback.questions(publicationId).then(setQuestions).catch(() => setQuestions([]))
    void api.feedback.answers(publicationId).then(setAnswers).catch(() => setAnswers([]))
  }, [publicationId, loadComments])

  useEffect(() => {
    api.publications.content(publicationId, page)
      .then((next) => {
        setContent(next)
        window.scrollTo({ top: 0 })
      })
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not open that chapter'))
  }, [publicationId, page])

  const goTo = (next: number) =>
    setParams((current) => {
      const updated = new URLSearchParams(current)
      if (next === 0) updated.delete('chapter')
      else updated.set('chapter', String(next))
      return updated
    })

  /** This reader's own marks, keyed the way the gutter wants them. */
  const marksByScene = useMemo(() => {
    const map = new Map<string, Record<number, ReactionKind>>()
    for (const block of reactions) {
      if (!block.mine) continue
      const existing = map.get(block.sceneId) ?? {}
      existing[block.blockIndex] = block.mine
      map.set(block.sceneId, existing)
    }
    return map
  }, [reactions])

  const react = useCallback(
    (sceneId: string, blockIndex: number, kind: ReactionKind) => {
      // Answered optimistically: a one-tap signal that waits for a round trip
      // before it looks pressed is not a one-tap signal.
      setReactions((current) => {
        const rest = current.filter((b) => !(b.sceneId === sceneId && b.blockIndex === blockIndex))
        const was = current.find((b) => b.sceneId === sceneId && b.blockIndex === blockIndex)
        if (was?.mine === kind) return rest
        return [...rest, { sceneId, blockIndex, quote: '', counts: [], mine: kind }]
      })
      void api.feedback
        .react(publicationId, { sceneId, blockIndex, kind })
        .then(() => setNotice(null))
        .catch((e) => {
          // The optimistic mark has to go back, and silently putting it back
          // would read as the button not working. A refusal here is usually the
          // reviewer cap, which is worth a sentence.
          setNotice(e instanceof Error ? e.message : 'That did not reach the author')
          void api.feedback.reactions(publicationId).then(setReactions).catch(() => undefined)
        })
    },
    [publicationId],
  )

  const answerFor = useCallback(
    (questionId: string, chapterId: string) =>
      answers.find((a) => a.questionId === questionId && a.chapterId === chapterId),
    [answers],
  )

  const recordAnswer = useCallback((answer: AnswerView) => {
    setAnswers((current) => [
      ...current.filter((a) => a.id !== answer.id),
      answer,
    ])
  }, [])

  /** Anchors grouped by scene, so each reader instance only paints its own. */
  const anchorsByScene = useMemo(() => {
    const map = new Map<string, AnchorSpec[]>()
    const walk = (list: CommentView[]) => {
      for (const comment of list) {
        const anchor = parseAnchor(comment.anchor)
        if (anchor && comment.sceneId) {
          const existing = map.get(comment.sceneId) ?? []
          existing.push({
            id: comment.id,
            from: anchor.from,
            to: anchor.to,
            resolved: comment.state === 'resolved',
          })
          map.set(comment.sceneId, existing)
        }
        walk(comment.replies)
      }
    }
    walk(comments)
    return map
  }, [comments])

  /** Reading order across the whole book, which is what a progress index
   *  counts along — not just the chapter on screen. */
  const sceneIds = useMemo(
    () => content?.toc?.flatMap((chapter) => chapter.sceneIds)
      ?? content?.chapters.flatMap((chapter) => chapter.scenes.map((scene) => scene.sceneId))
      ?? [],
    [content],
  )
  const chapterCount = content?.toc?.length ?? 1
  const lastPage = page >= chapterCount - 1

  // An author scrolling through their own draft is not a reader, and counting
  // them would put a 100% beside their own name on every publication.
  const isAuthor = !!user && card?.author.id === user.id
  useReadingProgress(publicationId, sceneIds, !!card && !isAuthor, content?.page ?? null)

  const chapterQuestions = questions.filter((q) => q.scope === 'chapter')
  const workQuestions = questions.filter((q) => q.scope === 'work')

  if (error) return <main className="review"><p className="error">{error}</p></main>
  if (!card || !content) return <main className="review"><p className="empty">Opening…</p></main>

  return (
    <main className="review">
      <section className="reader">
        <header className="reader__header">
          <h1>{card.workTitle}</h1>
          <p className="reader__meta">
            <Link to={`/people/${card.author.id}`}>{card.author.displayName}</Link> ·{' '}
            {card.versionLabel} ·{' '}
            {card.wordCount.toLocaleString()} words
            <span className={`badge badge--${card.visibility}`}>{card.visibility}</span>
            {readers > 1 && <span className="reader__live">{readers} reading now</span>}
          </p>
          {notice && (
            <p className="error" onClick={() => setNotice(null)}>
              {notice}
            </p>
          )}
        </header>

        {content.chapters.map((chapter) => (
          <section key={chapter.chapterId} className="reader__chapter">
            <h2>{chapter.title}</h2>
            {chapter.scenes.map((scene) => (
              <ReaderScene
                key={scene.sceneId}
                sceneId={scene.sceneId}
                title={scene.title}
                content={scene.content}
                anchors={anchorsByScene.get(scene.sceneId) ?? []}
                activeCommentId={activeCommentId}
                marks={marksByScene.get(scene.sceneId) ?? {}}
                onSelect={setPending}
                onActivateComment={setActiveCommentId}
                onReact={react}
              />
            ))}

            {!isAuthor && chapterQuestions.length > 0 && (
              <div className="questions">
                <h3>Before you go on</h3>
                {chapterQuestions.map((question) => (
                  <QuestionCard
                    key={`${question.id}:${chapter.chapterId}`}
                    question={question}
                    chapterId={chapter.chapterId}
                    existing={answerFor(question.id, chapter.chapterId)}
                    onAnswered={recordAnswer}
                  />
                ))}
              </div>
            )}
          </section>
        ))}

        {chapterCount > 1 && content.toc && (
          <nav className="reader__pages" aria-label="Chapters">
            <button type="button" disabled={page === 0} onClick={() => goTo(page - 1)}>
              ← {page > 0 ? content.toc[page - 1].title : 'Previous'}
            </button>
            <select value={page} onChange={(e) => goTo(Number(e.target.value))} aria-label="Go to chapter">
              {content.toc.map((chapter, i) => (
                <option key={chapter.chapterId} value={i}>
                  {i + 1}. {chapter.title}
                </option>
              ))}
            </select>
            <button type="button" disabled={lastPage} onClick={() => goTo(page + 1)}>
              {!lastPage ? content.toc[page + 1].title : 'Next'} →
            </button>
          </nav>
        )}

        {lastPage && !isAuthor && workQuestions.length > 0 && (
          <div className="questions questions--end">
            <h3>{card.author.displayName} would like to know</h3>
            {workQuestions.map((question) => (
              <QuestionCard
                key={question.id}
                question={question}
                chapterId=""
                existing={answerFor(question.id, '')}
                onAnswered={recordAnswer}
              />
            ))}
          </div>
        )}
      </section>

      <CommentSidebar
        publicationId={publicationId}
        comments={comments}
        pending={pending}
        isAuthor={isAuthor}
        viewerId={user?.id ?? null}
        activeCommentId={activeCommentId}
        reviewPrompt={card.reviewPrompt ?? null}
        filters={filters}
        onFilter={setFilters}
        onRefresh={loadComments}
        onActivate={setActiveCommentId}
        onClearPending={() => setPending(null)}
      />
    </main>
  )
}
