import { Link } from 'react-router-dom'
import { staff } from '../../api/client'
import { useLoad } from './common'

export function AdminHomePage() {
  const { data, error } = useLoad(() => staff.overview(), [])
  const card = (to: string, label: string, n: number | undefined, hint?: string) => (
    <Link to={to} className="admin__card">
      <strong>{n ?? '…'}</strong>
      <span>{label}</span>
      {hint && <small>{hint}</small>}
    </Link>
  )
  return (
    <main className="feed admin">
      <header className="feed__header"><h1>Site administration</h1></header>
      {error && <p className="error">{error}</p>}
      <div className="admin__cards">
        {card('/admin/queue', 'open in the queue', data?.queueOpen)}
        {card('/admin/queue?tab=held', 'held for review', data?.queueHeld)}
        {card('/admin/appeals', 'open appeals', data?.appealsOpen)}
        {card('/admin/accounts?status=restricted', 'restrictions ending in 24 hours', data?.endingSoon)}
        {card('/admin/accounts?sort=joined', 'sign-ups today', data?.signupsToday)}
        {card('/admin/queue', 'reports today', data?.reportsToday)}
        {(data?.waitingOnAppeal ?? 0) > 0 && card('/admin/appeals', 'accounts whose deletion is waiting on an appeal', data?.waitingOnAppeal)}
      </div>
      <p className="field__hint">
        You are signed in as {data?.role ?? '…'}. Administrators are made on the command line, never here.
      </p>
    </main>
  )
}
