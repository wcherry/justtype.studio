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
          <p className="field__hint">
            Version {report.version}, up for {since(report.uptimeSeconds)}. Checked every 30
            seconds while this page is open.
          </p>
        </>
      )}
    </main>
  )
}
