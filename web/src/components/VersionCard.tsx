import { useState } from 'react'
import { api } from '../api/client'
import { useStanding } from '../state/standing'
import type { VersionSummary } from '../api/types'

interface Props {
  version: VersionSummary
  busy: boolean
  onChanged: () => void
  onError: (message: string) => void
  onCompareWithDraft: () => void
  onExport: () => void
  onOffer: () => void
  children?: React.ReactNode
}

const STATE_LABEL: Record<VersionSummary['state'], string> = {
  draft: 'Draft',
  published: 'Published',
  final: 'Final',
}

/**
 * One snapshot on the shelf.
 *
 * A label chosen in the moment ("v3") is rarely the one that helps six months
 * later, so it can be renamed; the text it froze cannot.
 */
export function VersionCard({
  version, busy, onChanged, onError, onCompareWithDraft, onExport, onOffer, children,
}: Props) {
  const [renaming, setRenaming] = useState(false)
  const [tagging, setTagging] = useState(false)
  const [confirmRestore, setConfirmRestore] = useState(false)
  const { blocked } = useStanding()

  const run = async (work: () => Promise<unknown>) => {
    try {
      await work()
      onChanged()
    } catch (e) {
      onError(e instanceof Error ? e.message : 'Could not do that')
    }
  }

  return (
    <li className="versions__item">
      <div className="versions__row">
        <span className={`badge badge--${version.state}`}>{STATE_LABEL[version.state]}</span>
        {version.pinned && <span className="badge badge--pinned" title="Pinned">pinned</span>}
        {renaming ? (
          <RenameForm
            version={version}
            onDone={() => { setRenaming(false); onChanged() }}
            onCancel={() => setRenaming(false)}
          />
        ) : (
          <button type="button" className="versions__label" onClick={() => setRenaming(true)}>
            <strong>{version.label}</strong>
          </button>
        )}
        <span className="versions__meta">{version.wordCount.toLocaleString()} words</span>
      </div>

      <div className="versions__meta">
        {new Date(version.createdAt).toLocaleString()}
        {version.scope !== 'work' && <> · {version.scope} only</>}
      </div>
      {version.notes && <p className="versions__notes">{version.notes}</p>}

      <div className="versions__tags">
        {version.tags.map((tag) => (
          <button
            key={tag}
            type="button"
            className="tag"
            title="Remove this tag"
            onClick={() => run(() => api.versions.removeTag(version.id, tag))}
          >
            {tag} <span aria-hidden>×</span>
          </button>
        ))}
        {tagging ? (
          <TagForm
            versionId={version.id}
            onDone={() => { setTagging(false); onChanged() }}
            onCancel={() => setTagging(false)}
          />
        ) : (
          <button type="button" className="tag tag--add" onClick={() => setTagging(true)}>+ tag</button>
        )}
      </div>

      <div className="versions__actions">
        {version.state === 'draft' && (
          <button
            type="button"
            disabled={busy || !!blocked('publish')}
            title={blocked('publish') ?? undefined}
            onClick={() => run(() => api.versions.publish(version.id))}
          >
            Publish
          </button>
        )}
        {version.state === 'published' && (
          <>
            <button type="button" disabled={busy || !!blocked('publish')} title={blocked('publish') ?? undefined} onClick={onOffer}>Offer for review</button>
            <button type="button" disabled={busy} onClick={() => run(() => api.versions.finalize(version.id))}>
              Mark final
            </button>
          </>
        )}
        {!version.pinned && (
          <button type="button" disabled={busy} onClick={() => run(() => api.versions.pin(version.id))}>
            Pin
          </button>
        )}
        <button type="button" disabled={busy} onClick={onCompareWithDraft}>Compare with draft</button>
        <button type="button" disabled={busy} onClick={() => setConfirmRestore(true)}>Restore</button>
        <button type="button" onClick={onExport}>Export</button>
      </div>

      {confirmRestore && (
        <div className="restore">
          <p>
            This replaces your working draft with <strong>{version.label}</strong>. Your current
            draft is snapshotted first, so nothing is lost and you can restore it back.
          </p>
          <div className="restore__actions">
            <button
              type="button"
              disabled={busy}
              onClick={async () => {
                setConfirmRestore(false)
                await run(async () => {
                  const result = await api.versions.restore(version.id)
                  return result
                })
              }}
            >
              Restore it
            </button>
            <button type="button" onClick={() => setConfirmRestore(false)}>Cancel</button>
          </div>
        </div>
      )}

      {children}
    </li>
  )
}

function RenameForm({
  version, onDone, onCancel,
}: {
  version: VersionSummary
  onDone: () => void
  onCancel: () => void
}) {
  const [label, setLabel] = useState(version.label)
  const [notes, setNotes] = useState(version.notes ?? '')

  return (
    <form
      className="versions__rename"
      onSubmit={async (event) => {
        event.preventDefault()
        await api.versions.update(version.id, { label, notes })
        onDone()
      }}
    >
      <input autoFocus value={label} onChange={(e) => setLabel(e.target.value)} />
      <input
        value={notes}
        placeholder="What this one was for"
        onChange={(e) => setNotes(e.target.value)}
      />
      <button type="submit">Save</button>
      <button type="button" onClick={onCancel}>Cancel</button>
    </form>
  )
}

function TagForm({
  versionId, onDone, onCancel,
}: {
  versionId: string
  onDone: () => void
  onCancel: () => void
}) {
  const [tag, setTag] = useState('')
  return (
    <form
      className="versions__tag-form"
      onSubmit={async (event) => {
        event.preventDefault()
        if (tag.trim()) await api.versions.addTag(versionId, tag)
        onDone()
      }}
    >
      <input autoFocus value={tag} placeholder="submitted" onChange={(e) => setTag(e.target.value)} />
      <button type="submit">Add</button>
      <button type="button" onClick={onCancel}>×</button>
    </form>
  )
}
