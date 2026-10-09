import { useEffect, useRef, useState } from 'react'
import { downloadDraftExport, downloadExport } from '../api/client'
import type { Version } from '../api/types'

interface Props {
  /** A frozen version to export; or, without one, the current draft of `workId`. */
  version?: Version
  workId?: string
  onClose: () => void
}

type Format = 'docx' | 'epub' | 'pdf' | 'html' | 'txt' | 'project'
type Style = 'manuscript' | 'book'

const FORMATS: { value: Format; label: string; note: string; draftOnly?: boolean }[] = [
  { value: 'docx', label: 'Word (.docx)', note: 'What agents and magazines ask for' },
  { value: 'pdf', label: 'PDF', note: 'Courier, laid out as a submission' },
  { value: 'epub', label: 'EPUB', note: 'For beta readers on an e-reader' },
  { value: 'html', label: 'HTML', note: 'One web page, to read in a browser or publish' },
  { value: 'txt', label: 'Plain text', note: 'For pasting into a submission form' },
  {
    value: 'project',
    label: 'Project (.zip)',
    note:
      'The whole book: project.json, which rebuilds it in another Just Type Studio, and each scene as a Markdown file',
    draftOnly: true,
  },
]

/**
 * Exports of a version come from a frozen copy — a file you send someone has to
 * be a thing that stays the same. The current draft can be exported too, as it
 * reads at this moment, for when you want the book out now — or as a project,
 * to keep or to move to another instance.
 */
export function ExportDialog({ version, workId, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const [format, setFormat] = useState<Format>(version ? 'docx' : 'project')
  const [style, setStyle] = useState<Style>('manuscript')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    ref.current?.showModal()
  }, [])

  const options = FORMATS.filter((f) => !f.draftOnly || !version)
  const chosen = options.find((f) => f.value === format) ?? options[0]

  // PDF is Courier-only for now; saying so beats offering a choice that is
  // quietly ignored.
  const styleApplies = format === 'docx'

  return (
    <dialog
      ref={ref}
      className="dialog"
      aria-labelledby="export-title"
      onClose={onClose}
      // A click on the backdrop is a click on the dialog element itself.
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="export-title">{version ? `Export ${version.label}` : 'Export the current draft'}</h2>

      <label>
        Type
        <select value={chosen.value} onChange={(e) => setFormat(e.target.value as Format)}>
          {options.map((option) => (
            <option key={option.value} value={option.value}>{option.label}</option>
          ))}
        </select>
        <span className="field__hint">{chosen.note}</span>
      </label>

      {styleApplies && (
        <label>
          Laid out as
          <select value={style} onChange={(e) => setStyle(e.target.value as Style)}>
            <option value="manuscript">Standard manuscript format</option>
            <option value="book">A finished book</option>
          </select>
          <span className="field__hint">
            {style === 'manuscript'
              ? 'Courier, double spaced, # scene breaks, contact block and running header.'
              : 'Proportional type, chapter openings, a glyph for scene breaks.'}
          </span>
        </label>
      )}

      {error && <p className="error">{error}</p>}

      <div className="dialog__actions">
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            setError(null)
            try {
              if (version) await downloadExport(version.id, chosen.value, style)
              else if (workId) await downloadDraftExport(workId, chosen.value, style)
              ref.current?.close()
            } catch (e) {
              setError(e instanceof Error ? e.message : 'Could not build that file')
            } finally {
              setBusy(false)
            }
          }}
        >
          {busy ? 'Building…' : 'Download'}
        </button>
        <button type="button" onClick={() => ref.current?.close()}>Cancel</button>
      </div>
    </dialog>
  )
}
