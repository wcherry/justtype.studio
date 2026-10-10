import { useCallback, useEffect, useRef, useState } from 'react'
import type { Editor } from '@tiptap/react'
import type { DocNode, PromptEntryView } from '../api/types'
import type { ConflictDetails } from '../offline/queue'
import { entryKey, flushQueue, onQueueEvent, pendingCount, saveEntry } from '../offline/queue'
import type { SaveState } from './useAutosave'
import { localDay } from './useSessionGoals'

const AUTOSAVE_MS = 1500

/**
 * Saving a Daily Prompts answer: the scene editor's autosave, for one entry.
 *
 * The same debounce, the same offline queue (under `prompt:<day>`), the same
 * conflict check against the hash the text was built on. There are no
 * revisions: an answer keeps no history.
 */
export function useEntryAutosave({
  day,
  entry,
  editor,
  onSaved,
}: {
  day: string
  /** What the server last said, or null before the first save. */
  entry: PromptEntryView | null
  editor: Editor | null
  onSaved?: (entry: PromptEntryView) => void
}) {
  const [state, setState] = useState<SaveState>('idle')
  const [pending, setPending] = useState(0)
  const [conflict, setConflict] = useState<ConflictDetails | null>(null)
  const timer = useRef<number | null>(null)
  const inFlight = useRef(false)
  const baseHash = useRef<string | null>(entry?.contentHash ?? null)
  const key = entryKey(day)

  useEffect(() => {
    baseHash.current = entry?.contentHash ?? null
  }, [entry?.id, entry?.contentHash])

  const flush = useCallback(async () => {
    if (!editor || inFlight.current) return
    inFlight.current = true
    setState('saving')
    try {
      const saved = await saveEntry({
        kind: 'entry',
        sceneId: key,
        workId: '',
        sceneTitle: '',
        day,
        content: editor.getJSON() as DocNode,
        baseHash: baseHash.current,
        localDay: localDay(),
        queuedAt: Date.now(),
      })
      if (saved) {
        baseHash.current = saved.contentHash
        setState('saved')
        onSaved?.(saved)
        void pendingCount().then(setPending)
      } else {
        setState((s) => (s === 'conflict' ? s : 'queued'))
      }
    } catch {
      setState('error')
    } finally {
      inFlight.current = false
    }
  }, [editor, day, key, onSaved])

  const schedule = useCallback(() => {
    setState((s) => (s === 'conflict' ? s : 'dirty'))
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => void flush(), AUTOSAVE_MS)
  }, [flush])

  useEffect(() => {
    void pendingCount().then(setPending)
    return onQueueEvent((event) => {
      setPending(event.pending)
      if (event.sceneId !== key) return
      if (event.type === 'conflict') {
        setConflict(event.details)
        setState('conflict')
      } else if (event.type === 'entryFlushed') {
        baseHash.current = event.entry.contentHash
        setState('saved')
        onSaved?.(event.entry)
      } else if (event.type === 'failed') {
        setState('error')
      } else if (event.type === 'queued') {
        setState((s) => (s === 'conflict' ? s : 'queued'))
      }
    })
  }, [key, onSaved])

  useEffect(() => {
    const onHide = () => {
      if (state === 'dirty') void flush()
    }
    const onOnline = () => void flushQueue()
    window.addEventListener('beforeunload', onHide)
    window.addEventListener('online', onOnline)
    document.addEventListener('visibilitychange', onHide)
    return () => {
      window.removeEventListener('beforeunload', onHide)
      window.removeEventListener('online', onOnline)
      document.removeEventListener('visibilitychange', onHide)
    }
  }, [state, flush])

  useEffect(() => () => {
    if (timer.current) window.clearTimeout(timer.current)
  }, [])

  /** Keep this text over the one that changed elsewhere. */
  const keepMine = useCallback(async () => {
    if (!conflict) return
    baseHash.current = conflict.currentHash
    setConflict(null)
    await flush()
  }, [conflict, flush])

  return { state, pending, conflict, setConflict, schedule, flush, keepMine }
}
