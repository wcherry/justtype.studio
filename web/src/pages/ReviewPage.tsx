import { useCallback, useEffect, useMemo, useState } from 'react'
import { useParams } from 'react-router-dom'
import { api } from '../api/client'
import { parseAnchor } from '../api/types'
import type { CommentView, PublicationCard, VersionContent } from '../api/types'
import { CommentSidebar } from '../components/CommentSidebar'
import { ReaderScene } from '../components/ReaderScene'
import type { Selection } from '../components/ReaderScene'
import type { AnchorSpec } from '../editor/commentAnchors'
import { useReadingProgress } from '../reader/useReadingProgress'
import { useAuth } from '../state/auth'

export function ReviewPage() {
  const { publicationId = '' } = useParams()
  const { user } = useAuth()
  const [card, setCard] = useState<PublicationCard | null>(null)
  const [content, setContent] = useState<VersionContent | null>(null)
  const [comments, setComments] = useState<CommentView[]>([])
  const [pending, setPending] = useState<Selection | null>(null)
  const [activeCommentId, setActiveCommentId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const loadComments = useCallback(() => {
    api.comments.list(publicationId).then(setComments).catch(() => undefined)
  }, [publicationId])

  useEffect(() => {
    Promise.all([api.publications.get(publicationId), api.publications.content(publicationId)])
      .then(([nextCard, nextContent]) => {
        setCard(nextCard)
        setContent(nextContent)
      })
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not open that publication'))
    loadComments()
  }, [publicationId, loadComments])

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

  /** Reading order, which is what a progress index counts along. */
  const sceneIds = useMemo(
    () => content?.chapters.flatMap((chapter) => chapter.scenes.map((scene) => scene.sceneId)) ?? [],
    [content],
  )

  // An author scrolling through their own draft is not a reader, and counting
  // them would put a 100% beside their own name on every publication.
  const isAuthor = !!user && card?.author.id === user.id
  useReadingProgress(publicationId, sceneIds, !!card && !isAuthor)

  if (error) return <main className="review"><p className="error">{error}</p></main>
  if (!card || !content) return <main className="review"><p className="empty">Opening…</p></main>

  return (
    <main className="review">
      <section className="reader">
        <header className="reader__header">
          <h1>{card.workTitle}</h1>
          <p className="reader__meta">
            {card.author.displayName} · {card.versionLabel} ·{' '}
            {card.wordCount.toLocaleString()} words
            <span className={`badge badge--${card.visibility}`}>{card.visibility}</span>
          </p>
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
                onSelect={setPending}
                onActivateComment={setActiveCommentId}
              />
            ))}
          </section>
        ))}
      </section>

      <CommentSidebar
        publicationId={publicationId}
        comments={comments}
        pending={pending}
        isAuthor={isAuthor}
        activeCommentId={activeCommentId}
        reviewPrompt={card.reviewPrompt ?? null}
        onRefresh={loadComments}
        onActivate={setActiveCommentId}
        onClearPending={() => setPending(null)}
      />
    </main>
  )
}
