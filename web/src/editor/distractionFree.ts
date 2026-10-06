import { useCallback, useEffect, useState } from 'react'

/**
 * The page and nothing else.
 *
 * Puts away the shell, the outline, the header, the toolbar, the goals and
 * every panel, and asks the browser for the whole screen. What stays is the
 * prose, styled by the writer's own preferences — focus mode and typewriter
 * scrolling keep working underneath — plus anything that must never be hidden:
 * a save conflict, or being offline with work waiting. Hiding those is how a
 * writer loses words.
 *
 * Everything else comes back on request: the pointer at the top edge brings a
 * slim bar, ⌘⇧O the outline, and Escape or ⌘⇧F ends it.
 *
 * Done with a class on `<body>` rather than by re-rendering the page without
 * its parts, so the editor, its selection and its undo history are untouched
 * on the way in and out.
 */
export function useDistractionFree() {
  const [on, setOn] = useState(false)
  const [outline, setOutline] = useState(false)

  const enter = useCallback(() => {
    setOn(true)
    // Full screen is a bonus, not a requirement: some browsers refuse it
    // outside a click, and the mode works the same in a window.
    document.documentElement.requestFullscreen?.().catch(() => undefined)
  }, [])

  const leave = useCallback(() => {
    setOn(false)
    setOutline(false)
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined)
  }, [])

  useEffect(() => {
    document.body.classList.toggle('writing-page', on)
    document.body.classList.toggle('writing-page--outline', on && outline)
    return () => {
      document.body.classList.remove('writing-page', 'writing-page--outline')
    }
  }, [on, outline])

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey
      if (mod && event.shiftKey && event.key.toLowerCase() === 'f') {
        event.preventDefault()
        if (on) leave()
        else enter()
      } else if (on && mod && event.shiftKey && event.key.toLowerCase() === 'o') {
        event.preventDefault()
        setOutline((o) => !o)
      } else if (on && event.key === 'Escape' && !document.fullscreenElement) {
        leave()
      }
    }
    // Leaving full screen with the browser's own Escape leaves the page too:
    // one key, one meaning.
    const onFullscreen = () => {
      if (on && !document.fullscreenElement) leave()
    }
    window.addEventListener('keydown', onKey)
    document.addEventListener('fullscreenchange', onFullscreen)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.removeEventListener('fullscreenchange', onFullscreen)
    }
  }, [on, enter, leave])

  return { on, enter, leave, outline, toggleOutline: () => setOutline((o) => !o) }
}
