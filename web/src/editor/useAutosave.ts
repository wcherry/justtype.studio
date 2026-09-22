import { useCallback, useEffect, useRef, useState } from 'react'
import type { Editor } from '@tiptap/react'
import type { DocNode, Scene } from '../api/types'
import type { ConflictDetails } from '../offline/queue'
import { flushQueue, onQueueEvent, pendingCount, saveScene } from '../offline/queue'

export type SaveState = 'idle' | 'dirty' | 'saving' | 'saved' | 'queued' | 'conflict' | 'error'

const AUTOSAVE_MS = 1500
/** Autosaves are cheap; a checkpoint every half minute is what you can walk back to. */
const CHECKPOINT_MS = 30_000

interface Options {
  scene: Scene | null
  editor: Editor | null
  /** Called after a save the server accepted, so the outline can follow along. */
  onSaved?: (workWordCount: number) => void
}

/**
 * Two-tier saving: a fast debounced write that keeps the server copy current,
 * and a slower one that also asks the server to keep a revision. Losing work is
 * the one failure this product cannot have.
 *
 * Every write goes through the offline queue, so a disconnection is not an
 * error condition — it changes the status line from "Saved" to "Saved on this
 * device" and nothing else. The queue replays on reconnect.
 */
export function useAutosave({ scene, editor, onSaved }: Options) {
  const [state, setState] = useState<SaveState>('idle')
  const [wordCount, setWordCount] = useState(0)
  const [pending, setPending] = useState(0)
  const [conflict, setConflict] = useState<ConflictDetails | null>(null)

  const timer = useRef<number | null>(null)
  const lastCheckpoint = useRef(Date.now())
  const inFlight = useRef(false)
  /** The hash the text on screen was built on top of. */
  const baseHash = useRef<string | null>(null)
  /** Read inside callbacks that must not be re-created on every scene change. */
  const sceneRef = useRef<Scene | null>(null)
  sceneRef.current = scene

  // A new scene resets the baseline; nothing about the previous one carries over.
  useEffect(() => {
    baseHash.current = scene?.contentHash ?? null
    setWordCount(scene?.wordCount ?? 0)
    setConflict(null)
    setState('idle')
  }, [scene?.id, scene?.contentHash, scene?.wordCount])

  const flush = useCallback(
    async (createRevision: boolean) => {
      const current = sceneRef.current
      if (!current || !editor || inFlight.current) return
      inFlight.current = true
      setState('saving')
      try {
        const result = await saveScene(
          {
            sceneId: current.id,
            workId: current.workId,
            sceneTitle: current.title,
            content: editor.getJSON() as DocNode,
            baseHash: baseHash.current,
            queuedAt: Date.now(),
          },
          createRevision,
        )
        if (result) {
          // The server's hash becomes the new baseline, so the next save is
          // checked against what actually landed rather than what we sent.
          baseHash.current = result.scene.contentHash
          setWordCount(result.scene.wordCount)
          setState('saved')
          onSaved?.(result.workWordCount)
          // A direct save clears this scene's queue entry without going through
          // the replay path, so the count has to be re-read rather than waiting
          // for an event that will not come.
          void pendingCount().then(setPending)
          if (createRevision) lastCheckpoint.current = Date.now()
        } else {
          // Queued: either offline, or a conflict the event handler below picks
          // up. Either way the text is on disk and not lost.
          setState((s) => (s === 'conflict' ? s : 'queued'))
        }
      } catch {
        setState('error')
      } finally {
        inFlight.current = false
      }
    },
    [editor, onSaved],
  )

  const schedule = useCallback(() => {
    setState((s) => (s === 'conflict' ? s : 'dirty'))
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      void flush(Date.now() - lastCheckpoint.current > CHECKPOINT_MS)
    }, AUTOSAVE_MS)
  }, [flush])

  // Anything left queued by a previous session counts from the moment the editor
  // opens, not from the first event — otherwise a writer who reloads offline is
  // told nothing about the work still waiting to go up.
  useEffect(() => {
    void pendingCount().then(setPending)
  }, [])

  // Queue activity belongs on screen: a replayed save that succeeded while the
  // writer was looking elsewhere should still move the status line.
  useEffect(
    () =>
      onQueueEvent((event) => {
        setPending(event.pending)
        if (event.sceneId !== sceneRef.current?.id) return
        if (event.type === 'conflict') {
          setConflict(event.details)
          setState('conflict')
        } else if (event.type === 'flushed') {
          baseHash.current = event.result.scene.contentHash
          setWordCount(event.result.scene.wordCount)
          setState('saved')
          onSaved?.(event.result.workWordCount)
        } else if (event.type === 'failed') {
          setState('error')
        } else if (event.type === 'queued') {
          setState((s) => (s === 'conflict' ? s : 'queued'))
        }
      }),
    [onSaved],
  )

  // Never leave the page with a pending debounce, and catch up as soon as the
  // network comes back.
  useEffect(() => {
    const onHide = () => {
      if (state === 'dirty') void flush(true)
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

  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current)
    },
    [scene?.id],
  )

  /** After the writer resolves a conflict: adopt `hash` as the new baseline. */
  const resolveWith = useCallback(
    async (hash: string) => {
      baseHash.current = hash
      setConflict(null)
      setState('dirty')
      await flush(true)
    },
    [flush],
  )

  return { state, wordCount, setWordCount, pending, conflict, setConflict, schedule, flush, resolveWith }
}
