import { useState } from 'react'
import { downloadExport } from '../api/client'
import type { Version } from '../api/types'

interface Props {
  version: Version
  onClose: () => void
}

type Format = 'docx' | 'epub' | 'pdf' | 'txt'
type Style = 'manuscript' | 'book'

const FORMATS: { value: Format; label: string; note: string }[] = [
  { value: 'docx', label: 'Word (.docx)', note: 'What agents and magazines ask for' },
  { value: 'pdf', label: 'PDF', note: 'Courier, laid out as a submission' },
  { value: 'epub', label: 'EPUB', note: 'For beta readers on an e-reader' },
  { value: 'txt', label: 'Plain text', note: 'For pasting into a submission form' },
]

/**
 * Exports come from a frozen version, never the working copy — a file you send
 * someone has to be a thing that stays the same.
 */
export function ExportDialog({ version, onClose }: Props) {
  const [format, setFormat] = useState<Format>('docx')
  const [style, setStyle] = useState<Style>('manuscript')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // PDF is Courier-only for now; saying so beats offering a choice that is
  // quietly ignored.
  const styleApplies = format === 'docx'

  return (
    <div className="export">
      <h4>Export {version.label}</h4>

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
              await downloadExport(version.id, format, style)
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
