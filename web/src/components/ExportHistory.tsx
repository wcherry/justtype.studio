import { useEffect, useRef, useState } from 'react'
import { exporting } from '../api/client'
import type { ExportHistoryItem } from '../api/types'

interface Props {
  workId: string
  onClose: () => void
}

const when = (iso: string) =>
  new Date(iso).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })

const size = (bytes: number) =>
  bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${Math.round(bytes / 1024)} KB` : `${(bytes / 1024 / 1024).toFixed(1)} MB`

/**
 * What was sent, and when. A version's export can be made again and comes out
 * the same; a draft's cannot, since the draft has moved on, so only the record
 * is kept.
 */
export function ExportHistory({ workId, onClose }: Props) {
  const ref = useRef<HTMLDialogElement>(null)
  const [items, setItems] = useState<ExportHistoryItem[] | null>(null)
  const [message, setMessage] = useState<string | null>(null)

  useEffect(() => {
    ref.current?.showModal()
    void exporting.history(workId).then(setItems).catch((e: Error) => setMessage(e.message))
  }, [workId])

  return (
    <dialog
      ref={ref}
      className="dialog dialog--export"
      aria-labelledby="history-title"
      onClose={onClose}
      onClick={(e) => e.target === ref.current && ref.current?.close()}
    >
      <h2 id="history-title">Export history</h2>
      {items && items.length === 0 && <p className="field__hint">Nothing has been exported from this book yet.</p>}
      {items && items.length > 0 && (
        <table className="export-history">
          <thead>
            <tr><th>When</th><th>File</th><th>From</th><th /></tr>
          </thead>
          <tbody>
            {items.map((item) => (
              <tr key={item.id}>
                <td>{when(item.createdAt)}</td>
                <td>
                  {item.filename}
                  <span className="field__hint">
                    {item.format.toUpperCase()} · {size(item.byteSize)}
                    {item.profile ? ` · ${item.profile}` : ''}
                  </span>
                </td>
                <td>{item.versionLabel ?? 'Working draft'}</td>
                <td>
                  {item.canDownload ? (
                    <button
                      type="button"
                      onClick={async () => {
                        try {
                          const saved = await exporting.downloadAgain(workId, item.id)
                          setMessage(
                            saved.identical
                              ? 'The same file, byte for byte.'
                              : 'Made again from the frozen version. Details such as the cover may have changed since, so it is not identical.',
                          )
                        } catch (e) {
                          setMessage(e instanceof Error ? e.message : 'Could not make that file again')
                        }
                      }}
                    >
                      Download again
                    </button>
                  ) : (
                    <span className="field__hint">Record only</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {message && <p className="field__hint">{message}</p>}
      <div className="dialog__actions">
        <button type="button" onClick={() => ref.current?.close()}>Close</button>
      </div>
    </dialog>
  )
}
