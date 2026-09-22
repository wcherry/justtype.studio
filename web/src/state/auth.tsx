import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { api, tokens } from '../api/client'
import type { User } from '../api/types'

interface AuthContextValue {
  user: User | null
  loading: boolean
  login: (email: string, password: string) => Promise<void>
  register: (email: string, password: string, displayName: string) => Promise<void>
  logout: () => Promise<void>
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null)
  const [loading, setLoading] = useState(true)

  // A stored token may be stale; `me` is the cheapest way to find out.
  useEffect(() => {
    let cancelled = false
    if (!tokens.access()) {
      setLoading(false)
      return
    }
    api.auth
      .me()
      .then((me) => !cancelled && setUser(me))
      .catch(() => tokens.clear())
      .finally(() => !cancelled && setLoading(false))
    return () => {
      cancelled = true
    }
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const session = await api.auth.login(email, password)
    tokens.set(session)
    setUser(session.user)
  }, [])

  const register = useCallback(async (email: string, password: string, displayName: string) => {
    const session = await api.auth.register(email, password, displayName)
    tokens.set(session)
    setUser(session.user)
  }, [])

  const logout = useCallback(async () => {
    await api.auth.logout().catch(() => undefined)
    tokens.clear()
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
