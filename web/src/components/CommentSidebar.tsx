import { useState } from 'react'
import { api } from '../api/client'
import type { CommentCategory, CommentView } from '../api/types'
import type { Selection } from './ReaderScene'

interface Props {
  publicationId: string
  comments: CommentView[]
  pending: Selection | null
  isAuthor: boolean
  activeCommentId: string | null
  reviewPrompt: string | null
  onRefresh: () => void
  onActivate: (commentId: string | null) => void
  onClearPending: () => void
}

const CATEGORIES: CommentCategory[] = [
  'general', 'praise', 'plot', 'pacing', 'prose', 'character', 'typo',
]

export function CommentSidebar({
  publicationId, comments, pending, isAuthor, activeCommentId, reviewPrompt,
  onRefresh, onActivate, onClearPending,
}: Props) {
  const [sort, setSort] = useState<'top' | 'new'>('top')

  return (
    <aside className="comments">
      <header className="comments__header">
        <h3>Feedback</h3>
        <select value={sort} onChange={(e) => { setSort(e.target.value as 'top' | 'new'); onRefresh() }}>
          <option value="top">Most useful</option>
          <option value="new">Newest</option>
        </select>
      </header>

      {reviewPrompt && (
        <p className="comments__prompt">
          <strong>The author asked:</strong> {reviewPrompt}
        </p>
      )}

      {pending && <NewComment publicationId={publicationId} pending={pending} onDone={() => { onClearPending(); onRefresh() }} />}

      {comments.length === 0 && !pending && (
        <p className="empty">Select any passage to comment on it, or leave a general note below.</p>
      )}

      {!pending && (
        <NewComment publicationId={publicationId} pending={null} onDone={onRefresh} />
      )}

      <ul className="comments__list">
        {comments.map((comment) => (
          <CommentThread
            key={comment.id}
            comment={comment}
            publicationId={publicationId}
            isAuthor={isAuthor}
            activeCommentId={activeCommentId}
            onActivate={onActivate}
            onRefresh={onRefresh}
          />
        ))}
      </ul>
    </aside>
  )
}

function CommentThread({
  comment, publicationId, isAuthor, activeCommentId, onActivate, onRefresh, depth = 0,
}: {
  comment: CommentView
  publicationId: string
  isAuthor: boolean
  activeCommentId: string | null
  onActivate: (id: string | null) => void
  onRefresh: () => void
  depth?: number
}) {
  const [replying, setReplying] = useState(false)

  const vote = async (value: -1 | 1) => {
    // Sending the same value again clears it, which the server enforces too.
    await api.comments.vote(comment.id, comment.viewerVote === value ? 0 : value).catch(() => undefined)
    onRefresh()
  }

  return (
    <li
      className={[
        'comment',
        depth > 0 ? 'comment--reply' : '',
        comment.id === activeCommentId ? 'is-active' : '',
        comment.state === 'resolved' ? 'is-resolved' : '',
      ].filter(Boolean).join(' ')}
      onClick={() => onActivate(comment.id)}
    >
      <div className="comment__vote">
        <button type="button" className={comment.viewerVote === 1 ? 'is-on' : ''} onClick={() => vote(1)} title="Useful">▲</button>
        <span>{comment.score}</span>
        <button type="button" className={comment.viewerVote === -1 ? 'is-on' : ''} onClick={() => vote(-1)} title="Not useful">▼</button>
      </div>

      <div className="comment__body">
        <div className="comment__meta">
          <strong>{comment.author.displayName}</strong>
          <span className={`badge badge--${comment.category}`}>{comment.category}</span>
          {comment.state === 'resolved' && <span className="badge badge--resolved">resolved</span>}
        </div>
        {comment.anchorQuote && <blockquote className="comment__quote">{comment.anchorQuote}</blockquote>}
        <p>{comment.body}</p>
        <div className="comment__actions">
          <button type="button" onClick={() => setReplying((r) => !r)}>Reply</button>
          {isAuthor && comment.state === 'open' && (
            <button
              type="button"
              onClick={async () => {
                await api.comments.update(comment.id, { state: 'resolved' })
                onRefresh()
              }}
            >
              Resolve
            </button>
          )}
        </div>

        {replying && (
          <NewComment
            publicationId={publicationId}
            pending={null}
            parentId={comment.id}
            compact
            onDone={() => { setReplying(false); onRefresh() }}
          />
        )}

        {comment.replies.length > 0 && (
          <ul className="comments__list">
            {comment.replies.map((reply) => (
              <CommentThread
                key={reply.id}
                comment={reply}
                publicationId={publicationId}
                isAuthor={isAuthor}
                activeCommentId={activeCommentId}
                onActivate={onActivate}
                onRefresh={onRefresh}
                depth={depth + 1}
              />
            ))}
          </ul>
        )}
      </div>
    </li>
  )
}

function NewComment({
  publicationId, pending, parentId, compact, onDone,
}: {
  publicationId: string
  pending: Selection | null
  parentId?: string
  compact?: boolean
  onDone: () => void
}) {
  const [body, setBody] = useState('')
  const [category, setCategory] = useState<CommentCategory>('general')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!body.trim()) return
    setBusy(true)
    setError(null)
    try {
      await api.comments.create(publicationId, {
        body,
        parentId,
        category,
        sceneId: pending?.sceneId,
        anchor: pending ? { from: pending.from, to: pending.to } : undefined,
        anchorQuote: pending?.quote,
      })
      setBody('')
      onDone()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not post that comment')
    } finally {
      setBusy(false)
    }
  }

  return (
    <form className={compact ? 'new-comment new-comment--compact' : 'new-comment'} onSubmit={submit}>
      {pending && <blockquote className="comment__quote">{pending.quote}</blockquote>}
      <textarea
        rows={compact ? 2 : 3}
        value={body}
        placeholder={pending ? 'What about this passage?' : parentId ? 'Reply…' : 'Leave a general note'}
        onChange={(e) => setBody(e.target.value)}
      />
      <div className="new-comment__actions">
        {!parentId && (
          <select value={category} onChange={(e) => setCategory(e.target.value as CommentCategory)}>
            {CATEGORIES.map((c) => (
              <option key={c} value={c}>{c}</option>
            ))}
          </select>
        )}
        <button type="submit" disabled={busy || !body.trim()}>Post</button>
      </div>
      {error && <p className="error">{error}</p>}
    </form>
  )
}
