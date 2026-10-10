import { useEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { prompts } from '../api/client'
import type { ToWorkPreview } from '../api/types'

/**
 * Turning an answer into a project: a confirm step that shows what will be
 * made. The split itself is the server's (the same rules iOS gets), so the
 * preview here is exactly the book that will be created.
 */
export function ToWorkDialog({ entryId, onClose }: { entryId: string; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null)
  const navigate = useNavigate()
  const [preview, setPreview] = useState<ToWorkPreview | null>(null)
  const [title, setTitle] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    ref.current?.showModal()
    prompts
      .previewWork(entryId)
      .then((p) => {
        setPreview(p)
        setTitle(p.title)
      })
      .catch((e) => setError(e instanceof Error ? e.message : 'Could not read the answer'))
  }, [entryId])

  const create = async () => {
    setBusy(true)
    setError(null)
    try {
      const work = await prompts.toWork(entryId, title)
      ref.current?.close()
      navigate(`/works/${work.id}`)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not make the project')
      setBusy(false)
    }
  }

  const scenes = preview?.scenes ?? []
  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="to-work-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="to-work-title">Turn into a project</h2>
      <p className="field__hint">
        A new book of yours, with this answer copied into one chapter and the prompt saved in its notebook. The answer stays as it is.
      </p>
      <label>
        Title
        <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={200} />
      </label>
      {preview && (
        <>
          <p>
            <strong>{scenes.length === 1 ? '1 scene' : `${scenes.length} scenes`}</strong>, split at each line that is only{' '}
            <code>--</code>
            {preview.blankParagraphSplits ? ' and at each blank line.' : '.'}
          </p>
          <ol className="to-work__scenes">
            {scenes.map((s, i) => (
              <li key={i}>
                {s.firstLine || <em>No text</em>} <span className="field__hint">· {s.wordCount.toLocaleString()} words</span>
              </li>
            ))}
          </ol>
          {!preview.blankParagraphSplits && (
            <p className="field__hint">
              A blank line can separate scenes too: turn that on in <Link to="/settings/general">Settings</Link>.
            </p>
          )}
        </>
      )}
      {error && <p className="error">{error}</p>}
      <div className="dialog__actions">
        <button type="button" disabled={busy || !preview || scenes.length === 0 || !title.trim()} onClick={() => void create()}>
          {busy ? 'Making it…' : 'Make the project'}
        </button>
        <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
      </div>
    </dialog>
  )
}
