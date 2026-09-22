import { useCallback, useEffect, useRef, useState } from 'react'
import type { Editor } from '@tiptap/react'
import { api } from '../api/client'
import type { DocNode } from '../api/types'

export type SaveState = 'idle' | 'dirty' | 'saving' | 'saved' | 'error'

const AUTOSAVE_MS = 1500
/** Autosaves are cheap; a checkpoint every half minute is what you can walk back to. */
const CHECKPOINT_MS = 30_000

/**
 * Two-tier saving: a fast debounced write that keeps the server copy current,
 * and a slower one that also asks the server to keep a revision. Losing work is
 * the one failure this product cannot have.
 */
export function useAutosave(sceneId: string | null, editor: Editor | null) {
  const [state, setState] = useState<SaveState>('idle')
  const [wordCount, setWordCount] = useState(0)
  const timer = useRef<number | null>(null)
  const lastCheckpoint = useRef(Date.now())
  const inFlight = useRef(false)

  const flush = useCallback(
    async (createRevision: boolean) => {
      if (!sceneId || !editor || inFlight.current) return
      inFlight.current = true
      setState('saving')
      try {
        const result = await api.scenes.saveContent(
          sceneId,
          editor.getJSON() as DocNode,
          createRevision,
        )
        setWordCount(result.scene.wordCount)
        setState('saved')
        if (createRevision) lastCheckpoint.current = Date.now()
      } catch {
        setState('error')
      } finally {
        inFlight.current = false
      }
    },
    [sceneId, editor],
  )

  const schedule = useCallback(() => {
    setState('dirty')
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      void flush(Date.now() - lastCheckpoint.current > CHECKPOINT_MS)
    }, AUTOSAVE_MS)
  }, [flush])

  // Never leave the page with a pending debounce.
  useEffect(() => {
    const onHide = () => {
      if (state === 'dirty') void flush(true)
    }
    window.addEventListener('beforeunload', onHide)
    document.addEventListener('visibilitychange', onHide)
    return () => {
      window.removeEventListener('beforeunload', onHide)
      document.removeEventListener('visibilitychange', onHide)
    }
  }, [state, flush])

  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current)
    },
    [sceneId],
  )

  return { state, wordCount, setWordCount, schedule, flush }
}
