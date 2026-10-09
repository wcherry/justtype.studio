import { useState } from 'react'
import { downloadDraftExport, downloadExport } from '../api/client'
import type { Version } from '../api/types'

interface Props {
  /** A frozen version to export; or, without one, the current draft of `workId`. */
  version?: Version
  workId?: string
  onClose: () => void
}

type Format = 'docx' | 'epub' | 'pdf' | 'html' | 'txt'
type Style = 'manuscript' | 'book'

const FORMATS: { value: Format; label: string; note: string }[] = [
  { value: 'docx', label: 'Word (.docx)', note: 'What agents and magazines ask for' },
  { value: 'pdf', label: 'PDF', note: 'Courier, laid out as a submission' },
  { value: 'epub', label: 'EPUB', note: 'For beta readers on an e-reader' },
  { value: 'html', label: 'HTML', note: 'One web page, to read in a browser or publish' },
  { value: 'txt', label: 'Plain text', note: 'For pasting into a submission form' },
]

/**
 * Exports of a version come from a frozen copy — a file you send someone has to
 * be a thing that stays the same. The current draft can be exported too, as it
 * reads at this moment, for when you want the book out now.
 */
export function ExportDialog({ version, workId, onClose }: Props) {
  const [format, setFormat] = useState<Format>('docx')
  const [style, setStyle] = useState<Style>('manuscript')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // PDF is Courier-only for now; saying so beats offering a choice that is
  // quietly ignored.
  const styleApplies = format === 'docx'

  return (
    <div className="export">
      <h4>{version ? `Export ${version.label}` : 'Export the current draft'}</h4>

      <div className="export__formats">
        {FORMATS.map((option) => (
          <label key={option.value} className={format === option.value ? 'is-chosen' : undefined}>
            <input
              type="radio"
              name="format"
              value={option.value}
              checked={format === option.value}
              onChange={() => setFormat(option.value)}
            />
            <span>
              <strong>{option.label}</strong>
              <span className="field__hint">{option.note}</span>
            </span>
          </label>
        ))}
      </div>

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

      <div className="export__actions">
        <button
          type="button"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            setError(null)
            try {
              if (version) await downloadExport(version.id, format, style)
              else if (workId) await downloadDraftExport(workId, format, style)
            } catch (e) {
              setError(e instanceof Error ? e.message : 'Could not build that file')
            } finally {
              setBusy(false)
            }
          }}
        >
          {busy ? 'Building…' : 'Download'}
        </button>
        <button type="button" onClick={onClose}>Close</button>
      </div>
    </div>
  )
}
