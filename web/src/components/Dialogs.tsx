import { useEffect, useRef, useState } from 'react'

/**
 * Asks before something that cannot be undone. The button that does it says
 * what it does, and Cancel is where focus starts, so Enter on its own never
 * deletes anything.
 */
export function ConfirmDialog({
  title, children, action, confirmWord, onConfirm, onClose,
}: {
  title: string
  children: React.ReactNode
  /** What the button that goes ahead says: "Delete scene". */
  action: string
  /** For the gravest: the button waits until this is typed, exactly. */
  confirmWord?: string
  onConfirm: () => Promise<void> | void
  onClose: () => void
}) {
  const ref = useRef<HTMLDialogElement>(null)
  const cancelRef = useRef<HTMLButtonElement>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [typed, setTyped] = useState('')
  const typedOk = !confirmWord || typed.trim() === confirmWord.trim()

  useEffect(() => {
    ref.current?.showModal()
    cancelRef.current?.focus()
  }, [])

  return (
    <dialog
      ref={ref}
      className="dialog"
      role="alertdialog"
      aria-labelledby="confirm-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="confirm-title">{title}</h2>
      <div className="field__hint">{children}</div>
      {confirmWord && (
        <label>
          Type “{confirmWord}” to confirm
          <input value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        </label>
      )}
      {error && <p className="error">{error}</p>}
      <div className="dialog__actions">
        <button
          type="button"
          className="danger"
          disabled={busy || !typedOk}
          onClick={async () => {
            setBusy(true)
            setError(null)
            try {
              await onConfirm()
              ref.current?.close()
            } catch (e) {
              setError(e instanceof Error ? e.message : 'That did not work')
            } finally {
              setBusy(false)
            }
          }}
        >
          {busy ? 'Working…' : action}
        </button>
        <button ref={cancelRef} type="button" onClick={() => ref.current?.close()}>Cancel</button>
      </div>
    </dialog>
  )
}

/** Names a version of one scene: a point in its history to come back to. */
export function NameVersionDialog({
  sceneName, onSave, onClose,
}: {
  sceneName: string
  onSave: (label: string) => Promise<void>
  onClose: () => void
}) {
  const ref = useRef<HTMLDialogElement>(null)
  const [label, setLabel] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="version-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <form
        method="dialog"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            await onSave(label.trim())
            ref.current?.close()
          } catch (err) {
            setError(err instanceof Error ? err.message : 'Could not save that version')
          } finally {
            setBusy(false)
          }
        }}
      >
        <h2 id="version-title">Save a version of “{sceneName}”</h2>
        <p className="field__hint">
          The scene as it is now, kept under a name of your own and listed with the book's
          versions.
        </p>
        <label>
          Name
          <input
            autoFocus
            value={label}
            placeholder="Before the rewrite"
            maxLength={120}
            onChange={(e) => setLabel(e.target.value)}
          />
        </label>
        {error && <p className="error">{error}</p>}
        <div className="dialog__actions">
          <button type="submit" disabled={busy || !label.trim()}>{busy ? 'Saving…' : 'Save version'}</button>
          <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
        </div>
      </form>
    </dialog>
  )
}

/** A new name for something, with the old one to start from. */
export function RenameDialog({
  title, label = 'Title', initial, onSave, onClose,
}: {
  title: string
  label?: string
  initial: string
  onSave: (name: string) => Promise<void>
  onClose: () => void
}) {
  const ref = useRef<HTMLDialogElement>(null)
  const [name, setName] = useState(initial)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="rename-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <form
        method="dialog"
        onSubmit={async (e) => {
          e.preventDefault()
          setBusy(true)
          setError(null)
          try {
            await onSave(name.trim())
            ref.current?.close()
          } catch (err) {
            setError(err instanceof Error ? err.message : 'Could not rename that')
          } finally {
            setBusy(false)
          }
        }}
      >
        <h2 id="rename-title">{title}</h2>
        <label>
          {label}
          <input autoFocus value={name} onFocus={(e) => e.currentTarget.select()} onChange={(e) => setName(e.target.value)} />
        </label>
        {error && <p className="error">{error}</p>}
        <div className="dialog__actions">
          <button type="submit" disabled={busy || !name.trim() || name.trim() === initial.trim()}>
            {busy ? 'Saving…' : 'Rename'}
          </button>
          <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
        </div>
      </form>
    </dialog>
  )
}
