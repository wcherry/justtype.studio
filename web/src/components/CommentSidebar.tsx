import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router-dom'
import { api } from '../api/client'
import type { CommentFilters } from '../api/client'
import type { CommentCategory, CommentView, ReportReason } from '../api/types'
import type { Selection } from './ReaderScene'
import { useStanding } from '../state/standing'

interface Props {
  publicationId: string
  comments: CommentView[]
  pending: Selection | null
  isAuthor: boolean
  /** Who is reading, so nobody is offered a way to report themselves. */
  viewerId: string | null
  activeCommentId: string | null
  reviewPrompt: string | null
  filters: CommentFilters
  onFilter: (filters: CommentFilters) => void
  onRefresh: () => void
  onActivate: (commentId: string | null) => void
  onClearPending: () => void
  /** Bumped to put the cursor in the note for the selected passage. */
  composeFocus?: number
}

const CATEGORIES: CommentCategory[] = [
  'general', 'praise', 'plot', 'pacing', 'prose', 'character', 'typo',
]

const REPORT_REASONS: ReportReason[] = [
  'abuse', 'harassment', 'hate', 'sexual_content', 'self_harm', 'impersonation', 'illegal',
  'spam', 'spoiler', 'off_topic', 'other',
]

export function CommentSidebar({
  publicationId, comments, pending, isAuthor, viewerId, activeCommentId, reviewPrompt,
  filters, onFilter, onRefresh, onActivate, onClearPending, composeFocus = 0,
}: Props) {
  const [busy, setBusy] = useState(false)

  // The names in the thread, so "only Grace's notes" needs no second request.
  const reviewers = new Map<string, string>()
  const walk = (list: CommentView[]) => {
    for (const comment of list) {
      reviewers.set(comment.author.id, comment.author.displayName)
      walk(comment.replies)
    }
  }
  walk(comments)

  const set = (patch: CommentFilters) => onFilter({ ...filters, ...patch })

  return (
    <aside className="comments">
      <header className="comments__header">
        <h3>Feedback</h3>
        <select
          value={filters.sort ?? 'top'}
          onChange={(e) => set({ sort: e.target.value as 'top' | 'new' })}
        >
          <option value="top">Most useful</option>
          <option value="new">Newest</option>
        </select>
      </header>

      <div className="comments__filters">
        <select
          value={filters.category ?? ''}
          onChange={(e) => set({ category: (e.target.value || undefined) as CommentCategory | undefined })}
        >
          <option value="">Any kind</option>
          {CATEGORIES.map((c) => (
            <option key={c} value={c}>{c}</option>
          ))}
        </select>
        <select
          value={filters.state ?? ''}
          onChange={(e) => set({ state: (e.target.value || undefined) as 'open' | 'resolved' | undefined })}
        >
          <option value="">Open and resolved</option>
          <option value="open">Still open</option>
          <option value="resolved">Resolved</option>
        </select>
        {reviewers.size > 1 && (
          <select
            value={filters.authorId ?? ''}
            onChange={(e) => set({ authorId: e.target.value || undefined })}
          >
            <option value="">Everyone</option>
            {[...reviewers].map(([id, name]) => (
              <option key={id} value={id}>{name}</option>
            ))}
          </select>
        )}
        <label className="comments__agreed">
          <input
            type="checkbox"
            checked={filters.minScore != null}
            onChange={(e) => set({ minScore: e.target.checked ? 1 : undefined })}
          />
          Others agreed
        </label>
      </div>

      {/* Twenty typo reports are twenty rows and one decision. */}
      {isAuthor && comments.length > 1 && filters.state !== 'resolved' && (
        <button
          type="button"
          className="linklike comments__bulk"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              await api.comments.bulkState(publicationId, {
                state: 'resolved',
                commentIds: comments.map((c) => c.id),
              })
              onRefresh()
            } finally {
              setBusy(false)
            }
          }}
        >
          Resolve these {comments.length}
        </button>
      )}

      {reviewPrompt && (
        <p className="comments__prompt">
          <strong>The author asked:</strong> {reviewPrompt}
        </p>
      )}

      {pending && (
        <NewComment
          publicationId={publicationId}
          pending={pending}
          focusKey={composeFocus}
          onDone={() => { onClearPending(); onRefresh() }}
        />
      )}

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
            viewerId={viewerId}
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
  comment, publicationId, isAuthor, viewerId, activeCommentId, onActivate, onRefresh, depth = 0,
}: {
  comment: CommentView
  publicationId: string
  isAuthor: boolean
  viewerId: string | null
  activeCommentId: string | null
  onActivate: (id: string | null) => void
  onRefresh: () => void
  depth?: number
}) {
  const [replying, setReplying] = useState(false)
  const [reporting, setReporting] = useState(false)
  const [reason, setReason] = useState<ReportReason>('off_topic')
  const [done, setDone] = useState<string | null>(null)
  const removed = comment.state === 'deleted'
  const byModerator = comment.moderationState === 'removed'
  const { blocked } = useStanding()
  const cantDiscuss = blocked('discuss')

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
      {!removed && !byModerator && (
      <div className="comment__vote">
        <button type="button" disabled={!!cantDiscuss} className={comment.viewerVote === 1 ? 'is-on' : ''} onClick={() => vote(1)} title={cantDiscuss ?? 'Useful'}>▲</button>
        <span>{comment.score}</span>
        <button type="button" disabled={!!cantDiscuss} className={comment.viewerVote === -1 ? 'is-on' : ''} onClick={() => vote(-1)} title={cantDiscuss ?? 'Not useful'}>▼</button>
      </div>
      )}

      <div className="comment__body">
        {/* Kept only because someone answered it; the answers follow. */}
        {removed && <p className="comment__removed">This note was removed.</p>}
        {!removed && byModerator && <p className="comment__removed comment__moderated">Removed by a moderator.</p>}
        {!removed && !byModerator && (<>
        <div className="comment__meta">
          <strong><Link to={`/people/${comment.author.id}`}>{comment.author.displayName}</Link></strong>
          {/* What this reviewer's notes have been worth elsewhere, so a reader
              with a record of being right is legible as one. */}
          {comment.authorScore > 0 && (
            <span className="comment__standing" title="Votes, plus notes authors acted on">
              {comment.authorScore}
            </span>
          )}
          <span className={`badge badge--${comment.category}`}>{comment.category}</span>
          {comment.state === 'resolved' && <span className="badge badge--resolved">resolved</span>}
          {comment.moderationState === 'held' && <span className="badge" title="A person is checking it before others see it">awaiting review</span>}
        </div>
        {comment.anchorQuote && <blockquote className="comment__quote">{comment.anchorQuote}</blockquote>}
        <p>{comment.body}</p>
        <div className="comment__actions">
          <button type="button" disabled={!!cantDiscuss} title={cantDiscuss ?? undefined} onClick={() => setReplying((r) => !r)}>Reply</button>
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
          {isAuthor && (
            <button
              type="button"
              title="Put it on your revision list"
              onClick={async () => {
                await api.tasks.fromComment(comment.id)
                setDone('on your list')
              }}
            >
              Add to my list
            </button>
          )}
          {comment.author.id !== viewerId && (
            <button type="button" disabled={!!blocked('report')} title={blocked('report') ?? undefined} onClick={() => setReporting((r) => !r)}>Report</button>
          )}
          {done && <span className="field__hint">{done}</span>}
        </div>
        </>)}

        {reporting && (
          <form
            className="comment__report"
            onSubmit={async (event) => {
              event.preventDefault()
              await api.comments.report(comment.id, { reason })
              setReporting(false)
              setDone(reason === 'spoiler' || reason === 'off_topic' ? 'reported to the author' : 'reported to the author and the site’s staff')
            }}
          >
            <select value={reason} onChange={(e) => setReason(e.target.value as ReportReason)}>
              {REPORT_REASONS.map((r) => (
                <option key={r} value={r}>{r.replace('_', ' ')}</option>
              ))}
            </select>
            <button type="submit">Send</button>
          </form>
        )}

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
                viewerId={viewerId}
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
  publicationId, pending, parentId, compact, focusKey = 0, onDone,
}: {
  publicationId: string
  pending: Selection | null
  parentId?: string
  compact?: boolean
  focusKey?: number
  onDone: () => void
}) {
  const textarea = useRef<HTMLTextAreaElement>(null)
  useEffect(() => {
    if (focusKey === 0) return
    textarea.current?.focus()
    textarea.current?.scrollIntoView({ block: 'nearest' })
  }, [focusKey])
  const [body, setBody] = useState('')
  const [category, setCategory] = useState<CommentCategory>('general')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const cantPost = useStanding().blocked('discuss')

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
        ref={textarea}
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
        <button type="submit" disabled={busy || !body.trim() || !!cantPost} title={cantPost ?? undefined}>Post</button>
      </div>
      {cantPost && <p className="field__hint">{cantPost} <Link to="/standing">Why?</Link></p>}
      {error && <p className="error">{error}</p>}
    </form>
  )
}
