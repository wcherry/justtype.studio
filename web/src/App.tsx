import { Link, Navigate, Route, Routes, useLocation } from 'react-router-dom'
import { EditorPage } from './pages/EditorPage'
import { FeedPage } from './pages/FeedPage'
import { LibraryPage } from './pages/LibraryPage'
import { LoginPage } from './pages/LoginPage'
import { ProfilePage } from './pages/ProfilePage'
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
            <button type="button" className="linklike" onClick={() => void logout()}>Sign out</button>
          </>
        )}
      </nav>
      {children}
    </div>
  )
}

export function App() {
  return (
    <Routes>
      <Route path="/sign-in" element={<LoginPage />} />
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
