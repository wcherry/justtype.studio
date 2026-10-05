import { useEffect, useState } from 'react'
import { Link, Navigate, Route, Routes, useLocation } from 'react-router-dom'
import { api } from './api/client'
import { ForgotPage, ResetPage, TermsPage, VerifyPage } from './pages/AccountPages'
import { EditorPage } from './pages/EditorPage'
import { FeedPage } from './pages/FeedPage'
import { LibraryPage } from './pages/LibraryPage'
import { LoginPage } from './pages/LoginPage'
import { ProfilePage } from './pages/ProfilePage'
import { SettingsPage } from './pages/SettingsPage'
import { ReviewPage } from './pages/ReviewPage'
import { useAuth } from './state/auth'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { user, loading } = useAuth()
  const location = useLocation()
  if (loading) return <main className="empty">Loading…</main>
  if (!user) return <Navigate to="/sign-in" state={{ from: location }} replace />
  return <>{children}</>
}

function Chrome({ children }: { children: React.ReactNode }) {
  const { user, logout } = useAuth()
  return (
    <div className="shell">
      <nav className="shell__nav">
        <Link to="/" className="shell__brand">Scriptorium</Link>
        <Link to="/">Desk</Link>
        <Link to="/review">Review</Link>
        <span className="shell__spacer" />
        {user && (
          <>
            <Link to={`/people/${user.id}`} className="shell__user">{user.displayName}</Link>
            <Link to="/settings" className="shell__user">Account</Link>
            <button type="button" className="linklike" onClick={() => void logout()}>Sign out</button>
          </>
        )}
      </nav>
      <AccountBanners />
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

export function App() {
  return (
    <Routes>
      <Route path="/sign-in" element={<LoginPage />} />
      <Route path="/forgot" element={<ForgotPage />} />
      <Route path="/reset" element={<ResetPage />} />
      <Route path="/verify" element={<VerifyPage />} />
      <Route path="/terms" element={<TermsPage />} />
      <Route
        path="/settings"
        element={<RequireAuth><Chrome><SettingsPage /></Chrome></RequireAuth>}
      />
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
