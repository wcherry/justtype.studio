import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import type { Outline, Scene } from '../api/types'

interface Props {
  /** From inside a book: a document becomes a new scene in it. Without it, a new book. */
  into?: { workId: string }
  onBook?: (book: Outline) => void
  onScene?: (scene: Scene) => void
  onClose: () => void
}

/**
 * Brings a book, or a single document, in.
 *
 * A project zip made by Export → Project rebuilds the whole book; a Markdown
 * or text file is one document. From the library either makes a new book;
 * from inside a book a document is added to it as a scene.
 */
export function ImportDialog(props: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const [file, setFile] = useState<File | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const inside = props.into !== undefined

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  const submit = async () => {
    if (!file) return
    setBusy(true)
    setError(null)
    try {
      if (props.into) props.onScene?.(await api.works.importDocument(props.into.workId, file))
      else props.onBook?.(await api.works.import(file))
      ref.current?.close()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not import that file')
    } finally {
      setBusy(false)
    }
  }

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="import-title"
      onClose={props.onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="import-title">{inside ? 'Import a document' : 'Import'}</h2>
      <p className="field__hint">
        {inside
          ? 'A Markdown (.md) or text (.txt) file becomes a new scene at the end of this book, named for the file.'
          : 'A project (.zip) from Export → Project becomes a book with its chapters, scenes, details and notebook. A Markdown (.md) or text (.txt) document becomes a book of one scene.'}
      </p>
      <label>
        File
        <input
          type="file"
          accept={inside ? '.md,.markdown,.txt' : '.zip,.md,.markdown,.txt'}
          onChange={(e) => setFile(e.target.files?.[0] ?? null)}
        />
      </label>
      {error && <p className="error">{error}</p>}
      <div className="dialog__actions">
        <button type="button" disabled={busy || !file} onClick={() => void submit()}>
          {busy ? 'Importing…' : 'Import'}
        </button>
        <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
      </div>
    </dialog>
  )
}
