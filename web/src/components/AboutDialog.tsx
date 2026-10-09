import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'

/**
 * Who this is and which build is running, in a modal dialog. The version is the
 * one the service itself reports, so it is the build actually serving the page.
 */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null)
  const [version, setVersion] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    ref.current?.showModal()
    api.status().then((s) => setVersion(s.version)).catch(() => setFailed(true))
  }, [])

  return (
    <dialog
      ref={ref}
      className="about"
      aria-labelledby="about-title"
      onClose={onClose}
      // A click on the backdrop is a click on the dialog element itself.
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="about-title">Just Type Studio</h2>
      <p>{version ? `Version ${version}` : failed ? 'Version unavailable' : 'Version…'}</p>
      <p className="field__hint">
        A writing tool for long-form fiction, and a review surface for the readers you choose.
      </p>
      <form method="dialog">
        <button type="submit" autoFocus>Close</button>
      </form>
    </dialog>
  )
}
