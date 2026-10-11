import { Fragment, useEffect, useState } from 'react'
import { Link, NavLink, Navigate, Route, Routes, useLocation, useParams } from 'react-router-dom'
import { api } from './api/client'
import { ForgotPage, ResetPage, TermsPage, VerifyPage } from './pages/AccountPages'
import { EditorPage } from './pages/EditorPage'
import { FeedPage } from './pages/FeedPage'
import { LibraryPage } from './pages/LibraryPage'
import { LoginPage } from './pages/LoginPage'
import { ProfilePage } from './pages/ProfilePage'
import { SettingsPage } from './pages/SettingsPage'
import { StatusPage } from './pages/StatusPage'
import { ReviewPage } from './pages/ReviewPage'
import { AdminPromptsPage } from './pages/AdminPromptsPage'
import { AdminShell } from './pages/admin/AdminShell'
import { AdminHomePage } from './pages/admin/AdminHomePage'
import { AdminUsersPage } from './pages/admin/AdminUsersPage'
import { AdminUserPage } from './pages/admin/AdminUserPage'
import { AdminQueuePage } from './pages/admin/AdminQueuePage'
import { AdminAppealsPage } from './pages/admin/AdminAppealsPage'
import { AdminBansPage } from './pages/admin/AdminBansPage'
import { AdminWordsPage } from './pages/admin/AdminWordsPage'
import { AdminExternalPage } from './pages/admin/AdminExternalPage'
import { AdminLogPage } from './pages/admin/AdminLogPage'
import { AdminJobsPage } from './pages/admin/AdminJobsPage'
import { BanAppealPage, BanExportPage, SetupPage, StandingPage } from './pages/StandingPages'
import { RestrictionBanner } from './components/RestrictionBanner'
import { InboxPage } from './pages/InboxPage'
import { PromptDayPage, PromptsPage } from './pages/PromptsPage'
import { PromptEntryPage } from './pages/PromptEntryPage'
import { PromptWritePage } from './pages/PromptWritePage'
import { UnsubscribePage } from './pages/UnsubscribePage'
import { InviteButton } from './components/InviteButton'
import { useAuth } from './state/auth'
import { useUnreadCount } from './state/inbox'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth()
  const location = useLocation()
  if (loading) return <main className="empty">Loading…</main>
  if (!user) return <Navigate to="/sign-in" state={{ from: location }} replace />
  return <>{children}</>
}

function Chrome({ children }: { children: React.ReactNode }) {
  const { user, logout } = useAuth()
  const unread = useUnreadCount(!!user)
  return (
    <div className="shell">
      <nav className="shell__nav">
        <Link to="/" className="shell__brand">Just Type Studio</Link>
        <Link to="/">Desk</Link>
        <Link to="/review">Review</Link>
        {user && (
          <NavLink to="/prompts" className={({ isActive }) => (isActive ? 'is-active' : undefined)}>
            Daily Prompts
          </NavLink>
        )}
        <span className="shell__spacer" />
        {user && (
          <>
            <Link
              to="/inbox"
              className={unread > 0 ? 'shell__bell has-unread' : 'shell__bell'}
              aria-label={unread > 0 ? `Inbox, ${unread} unread` : 'Inbox'}
              title="Inbox"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0" />
              </svg>
              {unread > 0 && <span className="shell__count">{unread > 99 ? '99+' : unread}</span>}
            </Link>
            <InviteButton />
            <Link to={`/people/${user.id}`} className="shell__user">{user.displayName}</Link>
            {(user.isAdmin || user.isModerator) && (
              <Link to={user.isAdmin ? '/admin' : '/admin/queue'} className="shell__user">Admin</Link>
            )}
            <Link to="/settings" className="shell__user">Account</Link>
            <button type="button" className="linklike" onClick={() => void logout()}>Sign out</button>
          </>
        )}
      </nav>
      <AccountBanners />
      <RestrictionBanner />
      {children}
    </div>
  )
}

/**
 * What the account still owes: a confirmed address, agreement to terms that
 * have changed. Shown everywhere until done, because both decide what reaches
 * the writer.
 */
function AccountBanners() {
  const { user, updateUser } = useAuth()
  const [termsVersion, setTermsVersion] = useState<number | null>(null)
  const [sent, setSent] = useState(false)
  useEffect(() => {
    api.trust.terms().then((t) => setTermsVersion(t.version)).catch(() => undefined)
  }, [])
  if (!user) return null
  const outdated = termsVersion !== null && (user.termsVersion ?? 0) < termsVersion
  return (
    <>
      {!user.emailVerifiedAt && (
        <p className="banner">
          Confirm {user.email} — drafts and books shared with it reach you once you have.{' '}
          {sent ? 'Sent; check your mail.' : (
            <button
              type="button"
              className="linklike"
              onClick={() => api.account.resendVerification().then(() => setSent(true)).catch(() => setSent(true))}
            >
              Send the link again
            </button>
          )}
        </p>
      )}
      {outdated && (
        <p className="banner">
          The <Link to="/terms">terms</Link> have changed since you agreed to them.{' '}
          <button
            type="button"
            className="linklike"
            onClick={() => termsVersion && api.trust.acceptTerms(termsVersion).then(updateUser)}
          >
            Agree to version {termsVersion}
          </button>
        </p>
      )}
    </>
  )
}

/** A page that should start afresh when its route parameters change. */
function Keyed({ children }: { children: React.ReactNode }) {
  const params = useParams()
  return <Fragment key={JSON.stringify(params)}>{children}</Fragment>
}

export function App() {
  return (
    <Routes>
      <Route path="/sign-in" element={<LoginPage />} />
      <Route path="/forgot" element={<ForgotPage />} />
      <Route path="/reset" element={<ResetPage />} />
      <Route path="/verify" element={<VerifyPage />} />
      <Route path="/terms" element={<TermsPage />} />
      <Route path="/status" element={<StatusPage />} />
      <Route path="/setup" element={<SetupPage />} />
      <Route path="/ban/appeal" element={<BanAppealPage />} />
      <Route path="/ban/export" element={<BanExportPage />} />
      <Route
        path="/standing"
        element={<RequireAuth><Chrome><StandingPage /></Chrome></RequireAuth>}
      />
      <Route path="/prompts/unsubscribe" element={<UnsubscribePage />} />
      <Route path="/settings" element={<Navigate to="/settings/account" replace />} />
      <Route
        path="/settings/:tab"
        element={<RequireAuth><Chrome><SettingsPage /></Chrome></RequireAuth>}
      />
      <Route
        path="/prompts"
        element={<RequireAuth><Chrome><PromptsPage /></Chrome></RequireAuth>}
      />
      <Route
        path="/prompts/:day"
        element={<RequireAuth><Chrome><Keyed><PromptDayPage /></Keyed></Chrome></RequireAuth>}
      />
      <Route
        path="/prompts/:day/write"
        element={<RequireAuth><Chrome><Keyed><PromptWritePage /></Keyed></Chrome></RequireAuth>}
      />
      <Route
        path="/prompts/:day/entries/:entryId"
        element={<RequireAuth><Chrome><Keyed><PromptEntryPage /></Keyed></Chrome></RequireAuth>}
      />
      <Route
        path="/inbox"
        element={<RequireAuth><Chrome><InboxPage /></Chrome></RequireAuth>}
      />
      <Route path="/admin" element={<RequireAuth><Chrome><AdminShell /></Chrome></RequireAuth>}>
        <Route index element={<AdminHomePage />} />
        <Route path="queue" element={<AdminQueuePage />} />
        <Route path="accounts" element={<AdminUsersPage />} />
        <Route path="accounts/:userId" element={<Keyed><AdminUserPage /></Keyed>} />
        <Route path="appeals" element={<AdminAppealsPage />} />
        <Route path="bans" element={<AdminBansPage />} />
        <Route path="words" element={<AdminWordsPage />} />
        <Route path="external" element={<AdminExternalPage />} />
        <Route path="log" element={<AdminLogPage />} />
        <Route path="prompts" element={<AdminPromptsPage />} />
        <Route path="jobs" element={<AdminJobsPage />} />
      </Route>
      <Route
        path="/"
        element={<RequireAuth><Chrome><LibraryPage /></Chrome></RequireAuth>}
      />
      <Route
        path="/works/:workId"
        element={<RequireAuth><Chrome><EditorPage /></Chrome></RequireAuth>}
      />
      <Route
        path="/review"
        element={<RequireAuth><Chrome><FeedPage /></Chrome></RequireAuth>}
      />
      <Route
        path="/review/:publicationId"
        element={<RequireAuth><Chrome><ReviewPage /></Chrome></RequireAuth>}
      />
      <Route
        path="/people/:userId"
        element={<RequireAuth><Chrome><ProfilePage /></Chrome></RequireAuth>}
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
