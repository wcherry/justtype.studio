import { useEffect, useRef } from 'react'
import { api } from '../api/client'

const SETTLE_MS = 1200

/**
 * Reports how far down a publication a reader has actually got.
 *
 * It sends the deepest scene that has been on screen rather than the one
 * currently in view: someone who scrolls back up to re-read a line has still
 * been as far as they were. The server keeps its own furthest mark as well, so
 * a request lost to a closed tab costs nothing — the next scene supersedes it.
 */
export function useReadingProgress(publicationId: string, sceneIds: string[], enabled: boolean) {
  const sent = useRef(-1)
  // Effects compare dependencies by identity, and the caller rebuilds this list
  // on every render; the joined ids are the thing that actually changes.
  const key = sceneIds.join(',')

  useEffect(() => {
    if (!enabled || key === '') return
    const ids = key.split(',')
    let deepest = -1
    let timer: number | undefined

    const flush = () => {
      if (deepest <= sent.current) return
      const previous = sent.current
      const index = deepest
      sent.current = index
      void api.publications.recordProgress(publicationId, ids[index]).catch(() => {
        if (sent.current === index) sent.current = previous
      })
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue
          const index = ids.indexOf(entry.target.id.slice('scene-'.length))
          if (index > deepest) deepest = index
        }
        window.clearTimeout(timer)
        timer = window.setTimeout(flush, SETTLE_MS)
      },
      { threshold: 0.2 },
    )

    for (const id of ids) {
      const element = document.getElementById(`scene-${id}`)
      if (element) observer.observe(element)
    }

    return () => {
      observer.disconnect()
      window.clearTimeout(timer)
      flush()
    }
  }, [publicationId, key, enabled])
}
