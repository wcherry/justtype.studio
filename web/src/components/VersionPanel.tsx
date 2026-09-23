import { useState } from 'react'
import { api } from '../api/client'
import type { DiffResponse, Version, Visibility } from '../api/types'
import { DiffView } from './DiffView'
import { ExportDialog } from './ExportDialog'

interface Props {
  workId: string
  versions: Version[]
  onChanged: () => void
}

const STATE_LABEL: Record<Version['state'], string> = {
  draft: 'Draft',
  published: 'Published',
  final: 'Final',
}

/**
 * The version lifecycle in one place: snapshot the working copy, compare two
 * snapshots, publish one, offer it for review, or freeze it as final.
 */
export function VersionPanel({ workId, versions, onChanged }: Props) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [diff, setDiff] = useState<DiffResponse | null>(null)
  const [compare, setCompare] = useState<{ from?: string; to?: string }>({})
  const [offering, setOffering] = useState<string | null>(null)
  const [exporting, setExporting] = useState<string | null>(null)

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true)
    setError(null)
    try {
      await fn()
      onChanged()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Something went wrong')
    } finally {
      setBusy(false)
    }
  }

  const compareWithDraft = async (versionId: string) => {
    setBusy(true)
    try {
      setDiff(await api.versions.diff(workId, versionId, 'working'))
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not compare that')
    } finally {
      setBusy(false)
    }
  }

  const showDiff = async () => {
    if (!compare.from || !compare.to) return
    setBusy(true)
    try {
      setDiff(await api.versions.diff(workId, compare.from, compare.to))
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not compare those versions')
    } finally {
      setBusy(false)
    }
  }

  return (
    <aside className="versions">
      <header className="versions__header">
        <h3>Versions</h3>
        <button
          type="button"
          disabled={busy}
          onClick={() => run(() => api.versions.create(workId, window.prompt('Label for this snapshot') ?? undefined))}
        >
          Snapshot now
        </button>
      </header>

      {error && <p className="error">{error}</p>}

      <ul className="versions__list">
        {versions.map((version) => (
          <li key={version.id} className="versions__item">
            <div className="versions__row">
              <span className={`badge badge--${version.state}`}>{STATE_LABEL[version.state]}</span>
              <strong>{version.label}</strong>
              <span className="versions__meta">{version.wordCount.toLocaleString()} words</span>
            </div>
            <div className="versions__meta">{new Date(version.createdAt).toLocaleString()}</div>
            <div className="versions__actions">
              {version.state === 'draft' && (
                <button type="button" disabled={busy} onClick={() => run(() => api.versions.publish(version.id))}>
                  Publish
                </button>
              )}
              {version.state === 'published' && (
                <>
                  <button type="button" disabled={busy} onClick={() => setOffering(version.id)}>
                    Offer for review
                  </button>
                  <button type="button" disabled={busy} onClick={() => run(() => api.versions.finalize(version.id))}>
                    Mark final
                  </button>
                </>
              )}
              <button type="button" disabled={busy} onClick={() => compareWithDraft(version.id)}>
                Compare with draft
              </button>
              <button type="button" onClick={() => setExporting(exporting === version.id ? null : version.id)}>
                Export
              </button>
              <label className="versions__pick">
                <input
                  type="radio"
                  name="diff-from"
                  checked={compare.from === version.id}
                  onChange={() => setCompare((c) => ({ ...c, from: version.id }))}
                />
                from
              </label>
              <label className="versions__pick">
                <input
                  type="radio"
                  name="diff-to"
                  checked={compare.to === version.id}
                  onChange={() => setCompare((c) => ({ ...c, to: version.id }))}
                />
                to
              </label>
            </div>

            {exporting === version.id && (
              <ExportDialog version={version} onClose={() => setExporting(null)} />
            )}

            {offering === version.id && (
              <OfferForm
                onCancel={() => setOffering(null)}
                onSubmit={async (visibility, reviewPrompt) => {
                  await run(() => api.publications.create(version.id, { visibility, reviewPrompt }))
                  setOffering(null)
                }}
              />
            )}
          </li>
        ))}
      </ul>

      {versions.length === 0 && <p className="empty">No snapshots yet. Take one when a draft is worth keeping.</p>}

      <button type="button" disabled={busy || !compare.from || !compare.to} onClick={showDiff}>
        Compare selected
      </button>

      {diff && <DiffView diff={diff} />}
    </aside>
  )
}

function OfferForm({
  onSubmit,
  onCancel,
}: {
  onSubmit: (visibility: Visibility, reviewPrompt: string) => void
  onCancel: () => void
}) {
  const [visibility, setVisibility] = useState<Visibility>('platform')
  const [prompt, setPrompt] = useState('')

  return (
    <form
      className="offer"
      onSubmit={(event) => {
        event.preventDefault()
        onSubmit(visibility, prompt)
      }}
    >
      <label>
        Who can read it
        <select value={visibility} onChange={(e) => setVisibility(e.target.value as Visibility)}>
          <option value="unlisted">Anyone with the link</option>
          <option value="platform">Signed-in writers</option>
          <option value="public">Everyone</option>
        </select>
      </label>
      <label>
        What do you want feedback on?
        <textarea
          rows={3}
          value={prompt}
          placeholder="Does the opening earn the next page?"
          onChange={(e) => setPrompt(e.target.value)}
        />
      </label>
      <div className="offer__actions">
        <button type="submit">Offer for review</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}
