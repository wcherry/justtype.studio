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

const DB_NAME = 'scriptorium'
const DB_VERSION = 1
const OPEN_TIMEOUT_MS = 3_000
export const PENDING_SAVES = 'pendingSaves'

let dbPromise: Promise<IDBDatabase | null> | null = null

function openDatabase(): Promise<IDBDatabase | null> {
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
      request = indexedDB.open(DB_NAME, DB_VERSION)
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
    dbPromise = openDatabase().then((db) => {
      // Don't cache a failure forever: whatever was blocking may have cleared
      // by the next save, and retrying costs one request.
      if (!db) dbPromise = null
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
