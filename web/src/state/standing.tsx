import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import { standing as standingApi } from '../api/client'
import type { AccountStanding, MyRestriction } from '../api/types'
import { useAuth } from './auth'

/** What a restriction can take away. The same names the server refuses with. */
export type Capability = 'write' | 'publish' | 'discuss' | 'report' | 'invite' | 'follow'

/** Whether `r` takes `capability` away: the table in the restrictions ticket. */
function denies(r: MyRestriction, capability: Capability): boolean {
  if (r.kind === 'warning') return false
  if (r.scope === 'discussion') return ['discuss', 'report', 'invite', 'follow'].includes(capability)
  if (r.kind === 'suspend') return capability !== 'write'
  return true
}

/** "14 October 2026". */
export function longDate(iso: string): string {
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(undefined, { day: 'numeric', month: 'long', year: 'numeric' })
}

interface StandingValue {
  standing: AccountStanding | null
  refresh: () => void
  /** Why `capability` is off for this account, or null when it is on. */
  blocked: (capability: Capability) => string | null
}

const Context = createContext<StandingValue>({ standing: null, refresh: () => undefined, blocked: () => null })

export function StandingProvider({ children }: { children: ReactNode }) {
  const { user } = useAuth()
  const [standing, setStanding] = useState<AccountStanding | null>(null)
  const refresh = useCallback(() => {
    if (!user) {
      setStanding(null)
      return
    }
    standingApi.mine().then(setStanding).catch(() => undefined)
  }, [user])
  useEffect(() => refresh(), [refresh])

  const value = useMemo<StandingValue>(
    () => ({
      standing,
      refresh,
      blocked: (capability) => {
        const r = standing?.active.find((a) => denies(a, capability))
        if (!r) return null
        return r.endsAt
          ? `You can't do that until ${longDate(r.endsAt)}.`
          : "You can't do that on this account."
      },
    }),
    [standing, refresh],
  )
  return <Context.Provider value={value}>{children}</Context.Provider>
}

export function useStanding(): StandingValue {
  return useContext(Context)
}
