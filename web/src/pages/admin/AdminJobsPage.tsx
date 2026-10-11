import { admin } from '../../api/client'
import { errorText, useLoad, when } from './common'

/** Whether the scheduled work is running, what failed, and what the moderation platform has not answered. */
export function AdminJobsPage() {
  const { data, error } = useLoad(() => admin.jobs(), [])
  return (
    <main className="feed admin">
      <header className="feed__header"><h1>Jobs</h1></header>
      {error && <p className="error">{errorText(error)}</p>}
      {data && (
        <>
          <p className={data.health.ok ? 'banner' : 'error'}>
            {data.health.ok ? 'Everything that should have run has.' : data.health.problems.join(' · ')}
            {data.health.lastTick && ` Last tick ${when(data.health.lastTick)}.`}
          </p>
          <p>Mail waiting: {data.mailWaiting} · failed: {data.mailFailed}</p>
          <p>Posts waiting for the moderation platform: {data.externalBacklog}</p>
          {data.externalUnapplied.length > 0 && (
            <>
              <h2>Events from the platform that were stored and not applied</h2>
              <ul className="admin__list">{data.externalUnapplied.map((e, i) => <li key={i}>{e}</li>)}</ul>
            </>
          )}
          <h2>Recent runs</h2>
          <ul className="admin__list">
            {data.runs.map((r) => (
              <li key={`${r.job}-${r.key}`}>
                {r.job} · {r.key} · {when(r.startedAt)} {r.error ? <span className="error">— {r.error}</span> : r.finishedAt ? '✓' : '…'}
              </li>
            ))}
          </ul>
        </>
      )}
    </main>
  )
}
