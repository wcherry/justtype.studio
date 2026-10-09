import { useEffect, useRef, useState } from 'react'
import { APP_VERSION, BUILD_ID, FULL_VERSION_LABEL, VERSION_LABEL } from '../lib/version'

/**
 * Who this is and which build is running, in a modal dialog. A released image
 * shows its version and the commit; a local build shows the commit alone.
 */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    ref.current?.showModal()
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
      <p className="about__version">
        <span title={FULL_VERSION_LABEL || undefined}>{VERSION_LABEL || 'Development build'}</span>
        {FULL_VERSION_LABEL && (
          <button
            type="button"
            className="linklike"
            aria-label="Copy the version"
            title="Copy the version"
            onClick={() => {
              void navigator.clipboard?.writeText(FULL_VERSION_LABEL).then(() => {
                setCopied(true)
                setTimeout(() => setCopied(false), 1500)
              })
            }}
          >
            {copied ? 'Copied' : '⧉'}
          </button>
        )}
      </p>
      {APP_VERSION && BUILD_ID && <p className="field__hint">Build {BUILD_ID}</p>}
      <p className="field__hint">
        A writing tool for long-form fiction, and a review surface for the readers you choose.
      </p>
      <form method="dialog">
        <button type="submit" autoFocus>Close</button>
      </form>
    </dialog>
  )
}
