import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { ApiError, api, tokens } from '../api/client'
import type { User } from '../api/types'

/** Last known profile, so the app opens offline instead of bouncing to sign-in. */
const USER_KEY = 'scriptorium.user'

function cachedUser(): User | null {
  try {
    const raw = localStorage.getItem(USER_KEY)
    return raw ? (JSON.parse(raw) as User) : null
  } catch {
    return null
  }
}

function cacheUser(user: User | null) {
  try {
    if (user) localStorage.setItem(USER_KEY, JSON.stringify(user))
    else localStorage.removeItem(USER_KEY)
  } catch {
    // Private browsing or blocked site data: the session still works, it just
    // cannot be restored without the network.
  }
}

interface AuthContextValue {
  user: User | null
  loading: boolean
  login: (email: string, password: string) => Promise<void>
  register: (email: string, password: string, displayName: string) => Promise<void>
  logout: () => Promise<void>
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  // Start from the cached profile so a reload with no network opens the editor
  // rather than the sign-in page, then confirm it against the server.
  const [user, setUser] = useState<User | null>(() => (tokens.refresh() ? cachedUser() : null))
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    if (!tokens.access()) {
      setLoading(false)
      return
    }
    api.auth
      .me()
      .then((me) => {
        if (cancelled) return
        setUser(me)
        cacheUser(me)
      })
      .catch((error) => {
        // Only a real rejection ends the session. A network failure means the
        // server could not be asked, and signing the writer out over it would
        // strand unsynced work behind a login screen.
        if (error instanceof ApiError && error.status === 401) {
          tokens.clear()
          cacheUser(null)
          if (!cancelled) setUser(null)
        }
      })
      .finally(() => !cancelled && setLoading(false))
    return () => {
      cancelled = true
    }
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const session = await api.auth.login(email, password)
    tokens.set(session)
    cacheUser(session.user)
    setUser(session.user)
  }, [])

  const register = useCallback(async (email: string, password: string, displayName: string) => {
    const session = await api.auth.register(email, password, displayName)
    tokens.set(session)
    cacheUser(session.user)
    setUser(session.user)
  }, [])

  const logout = useCallback(async () => {
    await api.auth.logout().catch(() => undefined)
    tokens.clear()
    cacheUser(null)
    setUser(null)
  }, [])

  const value = useMemo(
    () => ({ user, loading, login, register, logout }),
    [user, loading, login, register, logout],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used inside an AuthProvider')
  return ctx
}
