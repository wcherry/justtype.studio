import { ApiError, api } from '../api/client'
import { prompts } from '../api/client'
import type { DocNode, PromptEntryView, SaveResult } from '../api/types'
import { signedInUserId } from '../state/auth'
import { idb } from './db'

/**
 * The offline write buffer.
 *
 * Every scene save goes through here. When the network is up this is a thin
 * pass-through; when it is down, the save lands in IndexedDB and is replayed on
 * reconnect. The editor never has to know which of the two happened, and a
 * writer who loses connectivity mid-sentence can keep typing.
 *
 * One entry per scene, holding the newest document. Replaying a queue of every
 * intermediate keystroke would send a hundred writes that the next one
 * immediately supersedes.
 *
 * A Daily Prompts answer queues the same way, under the key `prompt:<day>`
 * with `kind: 'entry'`, so a writer can answer offline and submit once back.
 */

export interface PendingSave {
  /** Unset on saves queued before answers existed: those are scenes. */
  kind?: 'scene' | 'entry'
  /** The key: a scene's id, or `prompt:<day>` for an answer. */
  sceneId: string
  workId: string
  sceneTitle: string
  content: DocNode
  /** Hash the queued edit was made on top of, carried so a replay is still conflict-checked. */
  baseHash: string | null
  /** The writer's local date when they wrote it — not when it finally syncs. */
  localDay: string
  queuedAt: number
  /** For an answer: the prompt's day. */
  day?: string
  /**
   * Who wrote it. Several writers can share one browser, and a save replayed
   * under someone else's session would be refused and dropped — or, for an
   * answer, filed under the wrong account. Unset on saves queued before this
   * existed; those replay for whoever is signed in, as they always did.
   */
  userId?: string
}

/** The saves the signed-in writer owns; anyone else's wait for them. */
async function ownSaves(): Promise<PendingSave[]> {
  const me = signedInUserId()
  return (await idb.all<PendingSave>()).filter((save) => !save.userId || save.userId === me)
}

/** The queue key an answer is saved under. */
export const entryKey = (day: string) => `prompt:${day}`

export type QueueEvent =
  | { type: 'queued'; sceneId: string; pending: number }
  | { type: 'flushed'; sceneId: string; result: SaveResult; pending: number }
  | { type: 'entryFlushed'; sceneId: string; entry: PromptEntryView; pending: number }
  | { type: 'conflict'; sceneId: string; pending: number; details: ConflictDetails }
  | { type: 'failed'; sceneId: string; pending: number; message: string }

export interface ConflictDetails {
  sceneId: string
  currentHash: string
  currentWordCount: number
  updatedAt: string
  yourHash: string
}

type Listener = (event: QueueEvent) => void

const listeners = new Set<Listener>()
let flushing = false

/**
 * `navigator.onLine` only knows whether the machine has a network interface, not
 * whether the server is reachable — a dropped VPN, a captive portal or a
 * restarting backend all look "online". So the queue retries on its own
 * schedule rather than waiting for an `online` event that will never fire.
 */
const RETRY_BASE_MS = 5_000
const RETRY_MAX_MS = 60_000
let retryDelay = RETRY_BASE_MS
let retryTimer: number | null = null

function scheduleRetry() {
  if (retryTimer !== null) return
  retryTimer = window.setTimeout(() => {
    retryTimer = null
    retryDelay = Math.min(retryDelay * 2, RETRY_MAX_MS)
    void flushQueue()
  }, retryDelay)
}

function cancelRetry() {
  if (retryTimer !== null) {
    window.clearTimeout(retryTimer)
    retryTimer = null
  }
  retryDelay = RETRY_BASE_MS
}

export function onQueueEvent(listener: Listener): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function emit(event: QueueEvent) {
  for (const listener of listeners) listener(event)
}

export async function pendingCount(): Promise<number> {
  return (await ownSaves()).length
}

export async function pendingFor(sceneId: string): Promise<PendingSave | null> {
  return idb.get<PendingSave>(sceneId)
}

async function enqueue(save: PendingSave) {
  await idb.put({ ...save, userId: save.userId ?? signedInUserId() ?? undefined })
  emit({ type: 'queued', sceneId: save.sceneId, pending: await pendingCount() })
}

/** A failure that leaves the write still worth retrying later. */
export function isOffline(error: unknown): boolean {
  if (error instanceof ApiError) {
    // 5xx and 408/429 are the server being temporarily unable, not a rejection
    // of the content. Anything else — 400, 403, 404, 409 — will fail the same
    // way on every retry, so queueing it would be an infinite loop.
    return error.status === 0 || error.status === 408 || error.status === 429 || error.status >= 500
  }
  return true // a TypeError from fetch is what "no network" looks like
}

/**
 * Save a scene, queueing it if the network is down.
 *
 * Returns the server's result when the write landed, or null when it was
 * queued — the caller shows "saved" or "saved locally" accordingly.
 */
export async function saveScene(save: PendingSave, createRevision: boolean): Promise<SaveResult | null> {
  if (typeof navigator !== 'undefined' && navigator.onLine === false) {
    await enqueue(save)
    return null
  }
  try {
    const result = await api.scenes.saveContent(
      save.sceneId,
      save.content,
      createRevision,
      save.baseHash,
      save.localDay,
    )
    await idb.delete(save.sceneId)
    return result
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      // Not retryable and not losable: hold the text so the writer can still
      // choose to keep it once they have seen what they collided with.
      await enqueue(save)
      emit({
        type: 'conflict',
        sceneId: save.sceneId,
        pending: await pendingCount(),
        details: error.details as ConflictDetails,
      })
      return null
    }
    if (isOffline(error)) {
      await enqueue(save)
      return null
    }
    throw error
  }
}

/**
 * Save an answer, queueing it if the network is down. The same contract as
 * `saveScene`: the server's view of it when it landed, null when queued.
 */
export async function saveEntry(save: PendingSave): Promise<PromptEntryView | null> {
  const queued = { ...save, kind: 'entry' as const }
  if (typeof navigator !== 'undefined' && navigator.onLine === false) {
    await enqueue(queued)
    return null
  }
  try {
    const entry = await prompts.saveEntry(save.day!, save.content, save.baseHash, save.localDay)
    await idb.delete(save.sceneId)
    return entry
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      await enqueue(queued)
      emit({
        type: 'conflict',
        sceneId: save.sceneId,
        pending: await pendingCount(),
        details: error.details as ConflictDetails,
      })
      return null
    }
    if (isOffline(error)) {
      await enqueue(queued)
      return null
    }
    throw error
  }
}

/**
 * Replay everything queued. Safe to call at any time — on reconnect, on load,
 * or on a timer — and it will not run twice concurrently.
 */
export async function flushQueue(): Promise<void> {
  if (flushing) return
  if (typeof navigator !== 'undefined' && navigator.onLine === false) {
    // Nothing to attempt now, but the browser will tell us when that changes.
    if (await pendingCount()) scheduleRetry()
    return
  }
  flushing = true
  let reachable = true
  try {
    for (const save of await ownSaves()) {
      try {
        if (save.kind === 'entry' && save.day) {
          const entry = await prompts.saveEntry(save.day, save.content, save.baseHash, save.localDay)
          await idb.delete(save.sceneId)
          emit({ type: 'entryFlushed', sceneId: save.sceneId, entry, pending: await pendingCount() })
          continue
        }
        const result = await api.scenes.saveContent(
          save.sceneId,
          save.content,
          // A queued save is by definition not a keystroke — it survived a
          // disconnection, so it is worth a revision of its own.
          true,
          save.baseHash,
          save.localDay,
        )
        await idb.delete(save.sceneId)
        emit({ type: 'flushed', sceneId: save.sceneId, result, pending: await pendingCount() })
      } catch (error) {
        if (error instanceof ApiError && error.status === 409) {
          emit({
            type: 'conflict',
            sceneId: save.sceneId,
            pending: await pendingCount(),
            details: error.details as ConflictDetails,
          })
          continue // stays queued until the writer resolves it
        }
        if (isOffline(error)) {
          reachable = false
          return // still down; the retry below picks it up
        }
        // A permanent rejection would otherwise block the queue forever.
        await idb.delete(save.sceneId)
        emit({
          type: 'failed',
          sceneId: save.sceneId,
          pending: await pendingCount(),
          message: error instanceof Error ? error.message : 'could not save',
        })
      }
    }
  } finally {
    flushing = false
    const remaining = await pendingCount()
    if (remaining > 0) {
      // A conflict waits on the writer, not on the network, so it must not keep
      // the backoff climbing — but anything else is worth another attempt.
      if (!reachable) scheduleRetry()
    } else {
      cancelRetry()
    }
  }
}

/** Drop a queued save, after the writer chooses to discard it. */
export async function discardPending(sceneId: string): Promise<void> {
  await idb.delete(sceneId)
}

/** Replay on reconnect, and once on load in case the tab was closed offline. */
export function startQueueReplay(): () => void {
  const onOnline = () => {
    cancelRetry()
    void flushQueue()
  }
  window.addEventListener('online', onOnline)
  void flushQueue()
  return () => {
    window.removeEventListener('online', onOnline)
    cancelRetry()
  }
}
