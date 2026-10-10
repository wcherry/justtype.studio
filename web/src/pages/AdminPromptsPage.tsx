import { useEffect, useState } from 'react'
import { ApiError, admin } from '../api/client'
import type { AdminPrompts, DailyPrompt, JobsReport } from '../api/types'
import { dayName } from '../prompts/days'

const SOURCE_LABEL: Record<string, string> = {
  curated: 'curated',
  ai: 'written by the model',
  evergreen: 'from the built-in list',
}

function PromptEditor({
  prompt,
  warning,
  onSave,
  onCancel,
}: {
  prompt: DailyPrompt
  warning?: string
  onSave: (body: string) => void
  onCancel: () => void
}) {
  const [body, setBody] = useState(prompt.body)
  return (
    <form
      className="admin__edit"
      onSubmit={(e) => {
        e.preventDefault()
        onSave(body)
      }}
    >
      <textarea aria-label="The prompt" rows={3} value={body} onChange={(e) => setBody(e.target.value)} />
      {warning && <p className="field__hint">{warning}</p>}
      <div className="critique__actions">
        <button type="submit" disabled={!body.trim()}>Save</button>
        <button type="button" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  )
}

/**
 * For site administrators: the prompts each day will get. The queue is used
 * in order, one a day, the day before; a prompt can instead be pinned to a
 * date. Tomorrow's is chosen at noon Pacific, and can be changed after.
 */
export function AdminPromptsPage() {
  const [page, setPage] = useState<AdminPrompts | null>(null)
  const [jobs, setJobs] = useState<JobsReport | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [forbidden, setForbidden] = useState(false)
  const [draft, setDraft] = useState('')
  const [pinDay, setPinDay] = useState('')
  const [editing, setEditing] = useState<string | null>(null)

  const run = async (work: () => Promise<AdminPrompts>) => {
    setError(null)
    try {
      setPage(await work())
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) setForbidden(true)
      setError(e instanceof Error ? e.message : 'Could not do that')
    }
  }

  useEffect(() => {
    void run(() => admin.prompts())
    admin.jobs().then(setJobs).catch(() => undefined)
  }, [])

  if (forbidden) {
    return (
      <main className="feed">
        <p className="error">This page is for site administrators.</p>
      </main>
    )
  }

  const queue = page?.queue ?? []
  const move = (index: number, by: number) => {
    const ids = queue.map((p) => p.id)
    const [id] = ids.splice(index, 1)
    ids.splice(index + by, 0, id)
    void run(() => admin.reorder(ids))
  }

  return (
    <main className="feed admin">
      <header className="feed__header">
        <h1>Daily prompts</h1>
      </header>
      {page?.warning && <p className="banner">{page.warning}</p>}
      {error && <p className="error">{error}</p>}

      <section className="prompts__section">
        <h2>Add a prompt</h2>
        <form
          className="admin__add"
          onSubmit={(e) => {
            e.preventDefault()
            void run(() => admin.addPrompt(draft, pinDay || null)).then(() => {
              setDraft('')
              setPinDay('')
            })
          }}
        >
          <textarea aria-label="New prompt" rows={3} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Write about…" />
          <div className="settings__row">
            <label>
              Pin to a day (optional)
              <input type="date" value={pinDay} min={page?.today} onChange={(e) => setPinDay(e.target.value)} />
            </label>
            <button type="submit" disabled={!draft.trim()}>{pinDay ? 'Pin it' : 'Add to the queue'}</button>
          </div>
        </form>
      </section>

      <section className="prompts__section">
        <h2>The queue ({queue.length})</h2>
        {queue.length === 0 && <p className="empty">Empty: the model will write each day's prompt the day before.</p>}
        <ol className="admin__queue">
          {queue.map((p, i) => (
            <li key={p.id}>
              {editing === p.id ? (
                <PromptEditor
                  prompt={p}
                  onCancel={() => setEditing(null)}
                  onSave={(body) => void run(() => admin.updatePrompt(p.id, { body })).then(() => setEditing(null))}
                />
              ) : (
                <>
                  <p>{p.body}</p>
                  <div className="comment__actions">
                    <button type="button" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move up">↑</button>
                    <button type="button" disabled={i === queue.length - 1} onClick={() => move(i, 1)} aria-label="Move down">↓</button>
                    <button type="button" onClick={() => setEditing(p.id)}>Edit</button>
                    <button
                      type="button"
                      onClick={() => {
                        const day = window.prompt('Pin to which day? (YYYY-MM-DD)')
                        if (day) void run(() => admin.updatePrompt(p.id, { day }))
                      }}
                    >
                      Pin to a day…
                    </button>
                    <button type="button" onClick={() => window.confirm('Delete this prompt?') && void run(() => admin.deletePrompt(p.id))}>
                      Delete
                    </button>
                  </div>
                </>
              )}
            </li>
          ))}
        </ol>
      </section>

      <section className="prompts__section">
        <h2>Days</h2>
        <ul className="admin__days">
          {(page?.days ?? []).map((d) => (
            <li key={d.day} className={d.day === page?.today ? 'is-today' : undefined}>
              <p className="feed__meta">
                {d.day === page?.today ? 'Today · ' : ''}
                {dayName(d.day, true)}
                {d.prompt && ` · ${SOURCE_LABEL[d.prompt.source] ?? d.prompt.source}`}
                {d.entries > 0 && ` · ${d.entries} answer${d.entries === 1 ? '' : 's'} started`}
              </p>
              {!d.prompt ? (
                <p className="field__hint">Nothing yet: it gets one from the queue the day before, at noon Pacific.</p>
              ) : editing === d.prompt.id ? (
                <PromptEditor
                  prompt={d.prompt}
                  warning={d.opened ? 'This day has opened: the change is recorded in the audit log, and writers may already be answering it.' : undefined}
                  onCancel={() => setEditing(null)}
                  onSave={(body) => void run(() => admin.updatePrompt(d.prompt!.id, { body })).then(() => setEditing(null))}
                />
              ) : (
                <>
                  <p>{d.prompt.body}</p>
                  <div className="comment__actions">
                    <button type="button" onClick={() => setEditing(d.prompt!.id)}>Edit</button>
                    {!d.opened && (
                      <button type="button" onClick={() => void run(() => admin.updatePrompt(d.prompt!.id, { unpin: true }))}>
                        Back to the queue
                      </button>
                    )}
                    {d.entries === 0 && (
                      <button type="button" onClick={() => window.confirm('Delete this prompt?') && void run(() => admin.deletePrompt(d.prompt!.id))}>
                        Delete
                      </button>
                    )}
                  </div>
                </>
              )}
            </li>
          ))}
        </ul>
      </section>

      {jobs && (
        <section className="prompts__section">
          <h2>Scheduled jobs</h2>
          <p className={jobs.health.ok ? 'field__hint' : 'error'}>
            {jobs.health.ok ? 'On schedule.' : jobs.health.problems.join(' ')}
            {jobs.health.lastTick && ` Last tick ${new Date(jobs.health.lastTick).toLocaleTimeString()}.`}
            {` Mail waiting: ${jobs.mailWaiting}; given up: ${jobs.mailFailed}.`}
          </p>
          <table className="export-history">
            <thead>
              <tr><th>Job</th><th>For</th><th>Started</th><th>Finished</th><th>Error</th></tr>
            </thead>
            <tbody>
              {jobs.runs.map((r) => (
                <tr key={`${r.job}-${r.key}`}>
                  <td>{r.job}</td>
                  <td>{r.key}</td>
                  <td>{new Date(r.startedAt).toLocaleString()}</td>
                  <td>{r.finishedAt ? new Date(r.finishedAt).toLocaleTimeString() : '—'}</td>
                  <td>{r.error ?? ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </main>
  )
}
