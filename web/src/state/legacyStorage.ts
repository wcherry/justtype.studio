/**
 * The product was called Scriptorium, and its browser storage was keyed
 * `scriptorium.*`. This moves anything under the old keys to the new ones,
 * once, before the app reads any of it — so a returning writer stays signed
 * in and keeps their preferences, cached scenes and share keys.
 *
 * A value already under the new key wins, and the old one is dropped either
 * way: there is no going back to the old name to need it.
 */
const OLD = 'scriptorium.'
const NEW = 'justtype.'

export function adoptLegacyStorage() {
  for (const storage of [safe(() => localStorage), safe(() => sessionStorage)]) {
    if (!storage) continue
    try {
      const keys: string[] = []
      for (let i = 0; i < storage.length; i++) {
        const key = storage.key(i)
        if (key?.startsWith(OLD)) keys.push(key)
      }
      for (const key of keys) {
        const renamed = NEW + key.slice(OLD.length)
        const value = storage.getItem(key)
        if (value !== null && storage.getItem(renamed) === null) storage.setItem(renamed, value)
        storage.removeItem(key)
      }
    } catch {
      // Blocked or full storage: the app starts as it would for a new visitor.
    }
  }
}

function safe(get: () => Storage): Storage | null {
  try {
    return get()
  } catch {
    return null
  }
}
