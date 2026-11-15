import { NavLink, Outlet } from 'react-router-dom'
import { staff } from '../../api/client'
import { useAuth } from '../../state/auth'
import { useLoad } from './common'

/**
 * The admin area: a side menu that shows only what the role allows, around
 * whichever page is open. Moderators see the queue, accounts, appeals and
 * the log; administrators see everything.
 */
export function AdminShell() {
  const { user } = useAuth()
  const isAdmin = !!user?.isAdmin
  const { data: overview, forbidden } = useLoad(() => staff.overview(), [])
  if (forbidden) {
    return (
      <main className="feed">
        <p className="error">This area is for the site's staff.</p>
      </main>
    )
  }
  const link = (to: string, label: string, badge?: number) => (
    <NavLink to={to} end={to === '/admin'} className={({ isActive }) => (isActive ? 'is-active' : undefined)}>
      {label}
      {badge ? <span className="admin__badge">{badge}</span> : null}
    </NavLink>
  )
  return (
    <div className="admin-shell">
      <nav className="admin-shell__menu" aria-label="Admin">
        {link('/admin', 'Overview')}
        {link('/admin/queue', 'Queue', overview?.queueOpen)}
        {link('/admin/accounts', 'Accounts')}
        {link('/admin/appeals', 'Appeals', overview?.appealsOpen)}
        {isAdmin && link('/admin/signups', 'Sign-ups')}
        {isAdmin && link('/admin/bans', 'Permanent bans')}
        {isAdmin && link('/admin/words', 'Word filter')}
        {isAdmin && link('/admin/external', 'External moderation')}
        {link('/admin/log', 'Log')}
        {isAdmin && link('/admin/prompts', 'Prompts')}
        {isAdmin && link('/admin/jobs', 'Jobs')}
      </nav>
      <div className="admin-shell__page">
        <Outlet />
      </div>
    </div>
  )
}
