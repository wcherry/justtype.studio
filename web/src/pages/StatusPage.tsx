import { useEffect, useState } from 'react'
import { api } from '../api/client'
import type { StatusReport } from '../api/types'

function since(seconds: number): string {
  if (seconds < 120) return `${seconds} seconds`
  if (seconds < 7200) return `${Math.round(seconds / 60)} minutes`
  if (seconds < 172800) return `${Math.round(seconds / 3600)} hours`
  return `${Math.round(seconds / 86400)} days`
}

/** The same checks an uptime monitor polls, for a person to read. */
export function StatusPage() {
  const [report, setReport] = useState<StatusReport | null>(null)
  const [unreachable, setUnreachable] = useState(false)

  useEffect(() => {
    const load = () =>
      api.status()
        .then((next) => { setReport(next); setUnreachable(false) })
        .catch(() => setUnreachable(true))
    load()
    const timer = setInterval(load, 30_000)
    return () => clearInterval(timer)
  }, [])

  return (
    <main className="feed status">
      <header className="feed__header">
        <h1>
          {unreachable
            ? 'Scriptorium is not answering'
            : !report
              ? 'Checking…'
              : report.status === 'ok'
                ? 'Everything is working'
                : 'Something needs looking at'}
        </h1>
      </header>
      {report && (
        <>
          <ul className="status__checks">
            {report.checks.map((check) => (
              <li key={check.name} className={check.ok ? 'is-ok' : 'is-failing'}>
                <span className="status__dot" aria-hidden>{check.ok ? '●' : '▲'}</span>
                <strong>{check.name}</strong>
                <span className="field__hint">{check.detail}</span>
              </li>
            ))}
          </ul>
          <h2 className="profile__heading">Write path</h2>
          <p className="field__hint">
            The architecture assumes a few writes a second against one SQLite file. These are
            the numbers that would say otherwise. Lock wait is sampled every five seconds.
          </p>
          <table className="status__table">
            <thead>
              <tr>
                <th />
                <th>Writes/s</th>
                <th>DB calls/s</th>
                <th>Write p50 / p95 / p99</th>
                <th>Lock wait p50 / p95 / max</th>
              </tr>
            </thead>
            <tbody>
              {report.writePath.map((w) => (
                <tr key={w.windowSeconds}>
                  <th>Last {w.windowSeconds >= 120 ? `${w.windowSeconds / 60} min` : 'minute'}</th>
                  <td>{w.writeRequestsPerSecond}</td>
                  <td>{w.dbWritesPerSecond}</td>
                  <td>{w.writeMs.p50} / {w.writeMs.p95} / {w.writeMs.p99} ms</td>
                  <td>{w.lockWaitMs.p50} / {w.lockWaitMs.p95} / {w.lockWaitMs.max} ms</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="field__hint">
            Version {report.version}, up for {since(report.uptimeSeconds)}. Checked every 30
            seconds while this page is open.
          </p>
        </>
      )}
    </main>
  )
}
