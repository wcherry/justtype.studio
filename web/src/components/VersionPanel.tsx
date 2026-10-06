import { useState } from 'react'
import { api } from '../api/client'
import type { DiffResponse, VersionSummary, Visibility } from '../api/types'
import { DiffView } from './DiffView'
import { ExportDialog } from './ExportDialog'
import { CloseButton } from './SidePanel'
import { VersionCard } from './VersionCard'

interface Props {
  workId: string
  versions: VersionSummary[]
  onChanged: () => void
  onClose?: () => void
}

/**
 * The version lifecycle in one place: snapshot the working copy, compare two
 * snapshots, publish one, offer it for review, or freeze it as final.
 */
export function VersionPanel({ workId, versions, onChanged, onClose }: Props) {
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
          onClick={() =>
            run(() =>
              api.versions.create(workId, window.prompt('Label for this snapshot') ?? undefined),
            )
          }
        >
          Snapshot now
        </button>
        {onClose && <CloseButton onClose={onClose} />}
      </header>

      {error && <p className="error">{error}</p>}

      <ul className="versions__list">
        {versions.map((version) => (
          <VersionCard
            key={version.id}
            version={version}
            busy={busy}
            onChanged={onChanged}
            onError={setError}
            onCompareWithDraft={() => compareWithDraft(version.id)}
            onExport={() => setExporting(exporting === version.id ? null : version.id)}
            onOffer={() => setOffering(version.id)}
          >
            <div className="versions__picks">
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
                onSubmit={async (visibility, reviewPrompt, closesAt, maxReviewers, spoilerSafe) => {
                  await run(() =>
                    api.publications.create(version.id, {
                      visibility,
                      reviewPrompt,
                      closesAt,
                      maxReviewers,
                      spoilerSafe,
                    }),
                  )
                  setOffering(null)
                }}
              />
            )}
          </VersionCard>
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
  onSubmit: (
    visibility: Visibility,
    reviewPrompt: string,
    closesAt?: string,
    maxReviewers?: number,
    spoilerSafe?: boolean,
  ) => void
  onCancel: () => void
}) {
  const [visibility, setVisibility] = useState<Visibility>('invited')
  const [prompt, setPrompt] = useState('')
  const [closesAt, setClosesAt] = useState('')
  const [cap, setCap] = useState('')
  const [spoilerSafe, setSpoilerSafe] = useState(false)

  return (
    <form
      className="offer"
      onSubmit={(event) => {
        event.preventDefault()
        onSubmit(
          visibility,
          prompt,
          // A date input gives a day; a review window closes at the end of it.
          closesAt ? new Date(`${closesAt}T23:59:59`).toISOString() : undefined,
          cap ? Number(cap) : undefined,
          spoilerSafe,
        )
      }}
    >
      <label>
        Who can read it
        <select value={visibility} onChange={(e) => setVisibility(e.target.value as Visibility)}>
          <option value="invited">Only readers I name</option>
          <option value="unlisted">Anyone with the link</option>
          <option value="platform">Signed-in writers</option>
          <option value="public">Everyone</option>
        </select>
      </label>
      <label>
        Closes on
        <input type="date" value={closesAt} onChange={(e) => setClosesAt(e.target.value)} />
        <span className="field__hint">Leave empty to keep it open.</span>
      </label>
      <label>
        At most
        <input
          type="number"
          min={1}
          value={cap}
          placeholder="no limit"
          onChange={(e) => setCap(e.target.value)}
        />
        <span className="field__hint">
          Readers who may comment. Beta readers are scarce, and twenty overlapping
          opinions are worse than five considered ones.
        </span>
      </label>
      <label className="offer__check">
        <input
          type="checkbox"
          checked={spoilerSafe}
          onChange={(e) => setSpoilerSafe(e.target.checked)}
        />
        Hold back notes on scenes a reader has not reached
        <span className="field__hint">
          For a serial, where not knowing yet is the point. On by default when
          the work is one.
        </span>
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
