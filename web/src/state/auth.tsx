import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { ApiError, api, tokens } from '../api/client'
import type { User } from '../api/types'
import { cache } from '../offline/cache'

/** Last known profile, so the app opens offline instead of bouncing to sign-in. */
const USER_KEY = 'justtype.user'

function cachedUser(): User | null {
  try {
    const raw = localStorage.getItem(USER_KEY)
    return raw ? (JSON.parse(raw) as User) : null
  } catch {
    return null
  }
}

/** Who is signed in on this browser, for code that runs outside React. */
export function signedInUserId(): string | null {
  return cachedUser()?.id ?? null
}

function cacheUser(user: User | null) {
  if (user) cache.claim(user.id)
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
  login: (email: string, password: string, code?: string) => Promise<void>
  register: (email: string, password: string, displayName: string, acceptTerms: boolean) => Promise<void>
  /** Re-reads the account, e.g. after the address has been proved elsewhere. */
  refreshUser: () => Promise<void>
  logout: () => Promise<void>
  /** After the profile is edited, so the name in the chrome follows it. */
  updateUser: (user: User) => void
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

  const login = useCallback(async (email: string, password: string, code?: string) => {
    const session = await api.auth.login(email, password, code)
    tokens.set(session)
    cacheUser(session.user)
    setUser(session.user)
  }, [])

  const register = useCallback(async (
    email: string, password: string, displayName: string, acceptTerms: boolean,
  ) => {
    const session = await api.auth.register(email, password, displayName, acceptTerms)
    tokens.set(session)
    cacheUser(session.user)
    setUser(session.user)
  }, [])

  const logout = useCallback(async () => {
    await api.auth.logout().catch(() => undefined)
    tokens.clear()
    cacheUser(null)
    cache.clear()
    setUser(null)
  }, [])

  const updateUser = useCallback((next: User) => {
    cacheUser(next)
    setUser(next)
  }, [])

  const refreshUser = useCallback(async () => {
    const me = await api.auth.me()
    cacheUser(me)
    setUser(me)
  }, [])

  const value = useMemo(
    () => ({ user, loading, login, register, logout, updateUser, refreshUser }),
    [user, loading, login, register, logout, updateUser, refreshUser],
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used inside an AuthProvider')
  return ctx
}
