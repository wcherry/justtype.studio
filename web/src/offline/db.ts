/**
 * A very small IndexedDB wrapper.
 *
 * Hand-rolled rather than pulled from npm: this needs one object store, four
 * operations and no schema migrations, and the callback-to-promise bridge below
 * is the entire reason libraries exist for this. A dependency here would be
 * more code to audit than to write.
 *
 * Every call resolves to a usable value even when IndexedDB is unavailable —
 * private browsing, blocked site data, an old WebView — because the editor must
 * keep working without it. The queue degrades to "saves go straight to the
 * network", which is exactly where it started.
 *
 * "Unavailable" includes never answering. An `open` request blocked behind an
 * unfinished `deleteDatabase`, or behind another tab holding an old version,
 * fires no event at all — not success, not error, not even `blocked`. Without
 * the timeout below, every caller awaits forever, and because opening a scene
 * checks the queue first, the editor simply never appears. A hang on a cache
 * lookup must never be able to take the editor down with it.
 */

const DB_NAME = 'justtype'
/** The database's name before the product was renamed; see `adoptLegacy`. */
const LEGACY_DB_NAME = 'scriptorium'
const DB_VERSION = 1
const OPEN_TIMEOUT_MS = 3_000
export const PENDING_SAVES = 'pendingSaves'

let dbPromise: Promise<IDBDatabase | null> | null = null

function openDatabase(name = DB_NAME, version: number | undefined = DB_VERSION): Promise<IDBDatabase | null> {
  if (typeof indexedDB === 'undefined') return Promise.resolve(null)
  return new Promise((resolve) => {
    let settled = false
    const settle = (db: IDBDatabase | null) => {
      if (settled) return
      settled = true
      resolve(db)
    }
    // The whole point: a request that never fires an event still has to end.
    const timer = setTimeout(() => settle(null), OPEN_TIMEOUT_MS)
    const finish = (db: IDBDatabase | null) => {
      clearTimeout(timer)
      settle(db)
    }

    let request: IDBOpenDBRequest
    try {
      request = indexedDB.open(name, version)
    } catch {
      finish(null)
      return
    }
    request.onupgradeneeded = () => {
      const db = request.result
      if (!db.objectStoreNames.contains(PENDING_SAVES)) {
        // Keyed by scene id: one pending save per scene, always the latest.
        // A log of every keystroke would replay a hundred stale writes on
        // reconnect, and only the last one can possibly be wanted.
        db.createObjectStore(PENDING_SAVES, { keyPath: 'sceneId' })
      }
    }
    request.onsuccess = () => finish(request.result)
    request.onerror = () => finish(null)
    request.onblocked = () => finish(null)
  })
}

function database(): Promise<IDBDatabase | null> {
  if (!dbPromise) {
    dbPromise = openDatabase().then(async (db) => {
      // Don't cache a failure forever: whatever was blocking may have cleared
      // by the next save, and retrying costs one request.
      if (!db) dbPromise = null
      else await adoptLegacy(db)
      return db
    })
  }
  return dbPromise
}

// `IDBRequest` is left unparameterised deliberately: it is invariant in its
// result type because of the `this` parameter on its event handlers, so a
// generic `IDBRequest<T>` here cannot accept the `IDBRequest<IDBValidKey>` that
// `put` returns or the `IDBRequest<undefined>` that `delete` does.
async function withStore<T>(
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => IDBRequest,
  fallback: T,
): Promise<T> {
  const db = await database()
  if (!db) return fallback
  return new Promise<T>((resolve) => {
    let request: IDBRequest
    try {
      request = run(db.transaction(PENDING_SAVES, mode).objectStore(PENDING_SAVES))
    } catch {
      resolve(fallback)
      return
    }
    request.onsuccess = () => resolve((request.result as T) ?? fallback)
    request.onerror = () => resolve(fallback)
  })
}

export const idb = {
  get: <T>(key: string) => withStore<T | null>('readonly', (s) => s.get(key), null),
  all: <T>() => withStore<T[]>('readonly', (s) => s.getAll(), []),
  put: <T>(value: T) => withStore<unknown>('readwrite', (s) => s.put(value), null),
  delete: (key: string) => withStore<unknown>('readwrite', (s) => s.delete(key), null),
}

/**
 * Moves saves still queued under the database's old name into this one, then
 * removes the old database. These can be writing that never reached the
 * server, so the old database is only deleted once every one of them has
 * been written here; a save already queued here for the same scene is newer
 * and is kept. Any failure leaves the old database where it is, to try again
 * on the next open.
 */
async function adoptLegacy(db: IDBDatabase): Promise<void> {
  try {
    // Opening a database that does not exist would create it, so look first.
    // Without `databases()` (older browsers) there is nothing safe to do.
    if (typeof indexedDB.databases !== 'function') return
    const names = (await indexedDB.databases()).map((d) => d.name)
    if (!names.includes(LEGACY_DB_NAME)) return
    const old = await openDatabase(LEGACY_DB_NAME, undefined)
    if (!old) return
    const saves = old.objectStoreNames.contains(PENDING_SAVES)
      ? await new Promise<{ sceneId: string }[] | null>((resolve) => {
          try {
            const request = old.transaction(PENDING_SAVES, 'readonly').objectStore(PENDING_SAVES).getAll()
            request.onsuccess = () => resolve(request.result as { sceneId: string }[])
            request.onerror = () => resolve(null)
          } catch {
            resolve(null)
          }
        })
      : []
    old.close()
    if (saves === null) return
    const copied = await new Promise<boolean>((resolve) => {
      try {
        const tx = db.transaction(PENDING_SAVES, 'readwrite')
        const store = tx.objectStore(PENDING_SAVES)
        for (const save of saves) {
          const existing = store.getKey(save.sceneId)
          existing.onsuccess = () => {
            if (existing.result === undefined) store.put(save)
          }
        }
        tx.oncomplete = () => resolve(true)
        tx.onerror = () => resolve(false)
        tx.onabort = () => resolve(false)
      } catch {
        resolve(false)
      }
    })
    if (copied) indexedDB.deleteDatabase(LEGACY_DB_NAME)
  } catch {
    // Leave the old database for the next open to try again.
  }
}

