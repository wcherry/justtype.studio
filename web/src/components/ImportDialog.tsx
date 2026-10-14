import { useEffect, useRef, useState, type DragEvent } from 'react'
import { api } from '../api/client'
import type { Outline, Scene } from '../api/types'
import { fromDroppedFolder, fromFolderInput } from '../lib/scrivener'

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
 * A project zip made by Export → Project rebuilds the whole book, and so does
 * a Scrivener project — a .scriv folder, picked or dropped here and zipped in
 * the browser, or a Scrivener backup zip. A Markdown or text file is one
 * document. From the library any of these makes a new book; from inside a book
 * a document is added to it as a scene.
 */
export function ImportDialog(props: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const [file, setFile] = useState<File | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [reading, setReading] = useState(false)
  const [dragging, setDragging] = useState(false)
  const inside = props.into !== undefined

  /** Zips a .scriv folder, however it arrived, into the file to send. */
  const gather = async (folder: () => Promise<File>) => {
    setReading(true)
    setError(null)
    setFile(null)
    try {
      setFile(await folder())
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not read that folder')
    } finally {
      setReading(false)
    }
  }

  const drop = (e: DragEvent) => {
    e.preventDefault()
    setDragging(false)
    const item = e.dataTransfer.items[0]
    const entry = item?.webkitGetAsEntry()
    if (entry?.isDirectory) {
      if (inside) setError('A folder is a whole project: import it from the library.')
      else void gather(() => fromDroppedFolder(entry as FileSystemDirectoryEntry))
      return
    }
    const dropped = e.dataTransfer.files[0]
    if (dropped) {
      setError(null)
      setFile(dropped)
    }
  }

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
      onDragOver={(e) => {
        e.preventDefault()
        setDragging(true)
      }}
      onDragLeave={(e) => e.target === ref.current && setDragging(false)}
      onDrop={drop}
      data-dragging={dragging || undefined}
    >
      <h2 id="import-title">{inside ? 'Import a document' : 'Import'}</h2>
      <p className="field__hint">
        {inside
          ? 'A Markdown (.md) or text (.txt) file becomes a new scene at the end of this book, named for the file.'
          : 'A project (.zip) from Export → Project becomes a book with its chapters, scenes, details and notebook. A Scrivener project (.scriv) becomes a book of its Draft, with its other documents in the notebook. A Markdown (.md) or text (.txt) document becomes a book of one scene.'}
      </p>
      <label>
        File
        <input
          type="file"
          accept={inside ? '.md,.markdown,.txt' : '.zip,.scriv,.md,.markdown,.txt'}
          onChange={(e) => {
            setError(null)
            setFile(e.target.files?.[0] ?? null)
          }}
        />
      </label>
      {!inside && (
        <label>
          Or a Scrivener project folder (.scriv)
          <input
            type="file"
            {...({ webkitdirectory: '' } as object)}
            onChange={(e) => {
              const files = e.target.files
              if (files?.length) void gather(() => fromFolderInput(files))
            }}
          />
          <span className="field__hint">You can also drag a .scriv from Finder or Explorer onto this window.</span>
        </label>
      )}
      {reading && <p className="field__hint">Reading the project…</p>}
      {file && !reading && <p className="field__hint">Ready: {file.name}</p>}
      {error && <p className="error">{error}</p>}
      <div className="dialog__actions">
        <button type="button" disabled={busy || reading || !file} onClick={() => void submit()}>
          {busy ? 'Importing…' : 'Import'}
        </button>
        <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
      </div>
    </dialog>
  )
}
