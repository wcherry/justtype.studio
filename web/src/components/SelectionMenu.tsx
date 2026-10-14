import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { Editor } from '@tiptap/react'
import { wordAt } from '../editor/spelling/spellcheck'

/** What was under the pointer when the menu was asked for. */
export interface MenuTarget {
  x: number
  y: number
  /** The selected passage, if the click was inside one. */
  selection: { from: number; to: number; quote: string } | null
  /** A highlight or bookmark the click landed on. */
  annotationId: string | null
  /** The word under the pointer, when nothing was selected there. */
  word: { from: number; to: number; text: string } | null
}

export interface SelectionAction {
  key: string
  label: string
  hint?: string
  /** A small heading above this item, starting a group. */
  heading?: string
  /** A line above this item. */
  divider?: boolean
  /** Shown, but not something to choose: "Looking…". */
  disabled?: boolean
  /** Opens a one-line form in the menu instead of acting at once. */
  compose?: { placeholder: string; submit: string; onSubmit: (text: string) => Promise<void> | void }
  onSelect?: () => void
}

/**
 * Listens on an editor for a right-click (or ⌘-click) on selected text, on
 * one of the person's own marks, or — with `words` — on any word, and
 * reports where.
 *
 * The menu key works too, on a selection made from the keyboard. Anything
 * else — a right-click between words, or with Shift held down — is left to
 * the browser, so its own menu is there when it is wanted.
 */
export function useSelectionMenu(editor: Editor | null, enabled = true, words = false) {
  const [target, setTarget] = useState<MenuTarget | null>(null)

  useEffect(() => {
    if (!editor || !enabled) return
    const dom = editor.view.dom

    const at = (event: MouseEvent): MenuTarget | null => {
      const markEl = (event.target as HTMLElement).closest('[data-annotation-id]')
      const annotationId = markEl?.getAttribute('data-annotation-id') ?? null
      const { from, to, empty } = editor.state.selection
      let selection: MenuTarget['selection'] = null
      if (!empty) {
        const pos = editor.view.posAtCoords({ left: event.clientX, top: event.clientY })
        if (pos && pos.pos >= from && pos.pos <= to) {
          selection = { from, to, quote: editor.state.doc.textBetween(from, to, ' ') }
        }
      }
      let word: MenuTarget['word'] = null
      if (!selection && words && event.type === 'contextmenu') {
        const pos = editor.view.posAtCoords({ left: event.clientX, top: event.clientY })
        word = pos ? wordAt(editor.state, pos.pos) : null
      }
      if (!selection && !annotationId && !word) return null
      return { x: event.clientX, y: event.clientY, selection, annotationId, word }
    }

    const onContextMenu = (event: MouseEvent) => {
      // From the keyboard (the menu key, or Shift+F10) there is no pointer:
      // the menu opens at the end of the selection instead.
      const fromKeyboard = 'pointerType' in event && (event as PointerEvent).pointerType === ''
      if (fromKeyboard && !editor.state.selection.empty) {
        const { from, to } = editor.state.selection
        const end = editor.view.coordsAtPos(to)
        event.preventDefault()
        setTarget({
          x: end.left,
          y: end.bottom + 4,
          selection: { from, to, quote: editor.state.doc.textBetween(from, to, ' ') },
          annotationId: null,
          word: null,
        })
        return
      }
      if (event.shiftKey) return
      const found = at(event)
      if (!found) return
      event.preventDefault()
      setTarget(found)
    }
    // ⌘-click inside a selection would otherwise collapse it before the menu
    // could use it, so the default is stopped on the way down.
    const onMouseDown = (event: MouseEvent) => {
      if (!event.metaKey || event.button !== 0) return
      const found = at(event)
      if (!found?.selection) return
      event.preventDefault()
      setTarget(found)
    }
    dom.addEventListener('contextmenu', onContextMenu)
    dom.addEventListener('mousedown', onMouseDown)
    return () => {
      dom.removeEventListener('contextmenu', onContextMenu)
      dom.removeEventListener('mousedown', onMouseDown)
    }
  }, [editor, enabled, words])

  return { target, close: () => setTarget(null) }
}

/** The menu itself, at the pointer, kept on screen. */
export function SelectionMenu({
  target, actions, onClose,
}: {
  target: MenuTarget
  actions: SelectionAction[]
  onClose: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [composing, setComposing] = useState<SelectionAction | null>(null)
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [place, setPlace] = useState({ left: target.x, top: target.y })

  // Placed again whenever it changes size: corrections and synonyms arrive
  // after it opens, and would otherwise push it off the bottom of the window.
  // Without room below the pointer it opens above it, as the browser's does.
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const fit = () => {
      const { width, height } = el.getBoundingClientRect()
      const below = target.y + height <= window.innerHeight - 8
      const top = below || target.y - height < 8 ? target.y : target.y - height
      setPlace({
        left: Math.max(8, Math.min(target.x, window.innerWidth - width - 8)),
        top: Math.max(8, Math.min(top, window.innerHeight - height - 8)),
      })
    }
    fit()
    const observer = new ResizeObserver(fit)
    observer.observe(el)
    return () => observer.disconnect()
  }, [target, composing])

  useEffect(() => {
    if (!composing) ref.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus()
  }, [composing])

  useEffect(() => {
    const away = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose()
    }
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    // Scrolling would leave the menu pointing at the wrong words.
    const scrolled = (event: Event) => {
      if (!ref.current?.contains(event.target as Node)) onClose()
    }
    document.addEventListener('pointerdown', away)
    document.addEventListener('keydown', escape)
    window.addEventListener('scroll', scrolled, true)
    return () => {
      document.removeEventListener('pointerdown', away)
      document.removeEventListener('keydown', escape)
      window.removeEventListener('scroll', scrolled, true)
    }
  }, [onClose])

  const move = (event: React.KeyboardEvent, by: number) => {
    event.preventDefault()
    const items = [...(ref.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [])]
    const at = items.indexOf(document.activeElement as HTMLButtonElement)
    items[(at + by + items.length) % items.length]?.focus()
  }

  return (
    <div
      ref={ref}
      className="selection-menu"
      style={{ left: place.left, top: place.top }}
      role={composing ? 'dialog' : 'menu'}
      aria-label={composing ? composing.label : 'Selected text'}
      onKeyDown={(event) => {
        if (composing) return
        if (event.key === 'ArrowDown') move(event, 1)
        else if (event.key === 'ArrowUp') move(event, -1)
      }}
    >
      {composing?.compose ? (
        <form
          className="selection-menu__compose"
          onSubmit={async (event) => {
            event.preventDefault()
            if (!text.trim()) return
            setBusy(true)
            setError(null)
            try {
              await composing.compose!.onSubmit(text.trim())
              onClose()
            } catch (e) {
              setError(e instanceof Error ? e.message : 'That did not go through')
              setBusy(false)
            }
          }}
        >
          {target.selection && (
            <blockquote className="comment__quote">{clip(target.selection.quote)}</blockquote>
          )}
          <textarea
            rows={3}
            autoFocus
            value={text}
            placeholder={composing.compose.placeholder}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) e.currentTarget.form?.requestSubmit()
            }}
          />
          {error && <p className="error">{error}</p>}
          <div className="selection-menu__actions">
            <button type="button" className="linklike" onClick={onClose}>Cancel</button>
            <button type="submit" disabled={busy || !text.trim()}>{composing.compose.submit}</button>
          </div>
        </form>
      ) : (
        actions.map((action) => [
          action.heading && (
            <div key={`${action.key}-heading`} className={`menu__heading${action.divider ? ' has-divider' : ''}`}>
              {action.heading}
            </div>
          ),
          <button
            key={action.key}
            type="button"
            role="menuitem"
            className={`menu__item${action.divider && !action.heading ? ' has-divider' : ''}`}
            disabled={action.disabled}
            onClick={() => {
              if (action.compose) {
                setComposing(action)
                return
              }
              onClose()
              action.onSelect?.()
            }}
          >
            <span className="menu__check" aria-hidden="true" />
            <span className="menu__label">{action.label}</span>
            {action.hint && <span className="menu__hint">{action.hint}</span>}
          </button>,
        ])
      )}
    </div>
  )
}

function clip(quote: string) {
  return quote.length > 160 ? `${quote.slice(0, 160).trimEnd()}…` : quote
}
