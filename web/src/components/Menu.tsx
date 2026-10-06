import { useEffect, useId, useRef, useState, type ReactNode } from 'react'

export interface MenuItem {
  key: string
  label: string
  /** Shown on the right, e.g. a shortcut. */
  hint?: string
  /** Marks the item whose panel is open. */
  checked?: boolean
  disabled?: boolean
  onSelect: () => void
}

/**
 * A button that opens a short list of actions.
 *
 * Keyboard as a menu should be: Enter, Space or ↓ opens it on the first item,
 * ↑ on the last; the arrows, Home and End move; Escape or Tab closes it and
 * focus goes back to the button. A click anywhere else closes it too.
 */
export function Menu({
  label, items, active = false, title,
}: {
  label: ReactNode
  items: MenuItem[]
  /** One of its items is open, so the button shows it. */
  active?: boolean
  title?: string
}) {
  const [open, setOpen] = useState(false)
  const [focusIndex, setFocusIndex] = useState(0)
  const rootRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([])
  const id = useId()
  const usable = items.map((item, index) => (item.disabled ? -1 : index)).filter((i) => i >= 0)

  useEffect(() => {
    if (!open) return
    const away = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false)
    }
    document.addEventListener('pointerdown', away)
    return () => document.removeEventListener('pointerdown', away)
  }, [open])

  useEffect(() => {
    if (open) itemRefs.current[focusIndex]?.focus()
  }, [open, focusIndex])

  const openAt = (which: 'first' | 'last') => {
    if (usable.length === 0) return
    setFocusIndex(which === 'first' ? usable[0] : usable[usable.length - 1])
    setOpen(true)
  }

  const close = (refocus: boolean) => {
    setOpen(false)
    if (refocus) buttonRef.current?.focus()
  }

  const step = (by: number) => {
    const at = usable.indexOf(focusIndex)
    setFocusIndex(usable[(at + by + usable.length) % usable.length])
  }

  return (
    <div className="menu" ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        id={`${id}-button`}
        className={active ? 'menu__button is-active' : 'menu__button'}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? `${id}-list` : undefined}
        title={title}
        onClick={() => (open ? close(false) : openAt('first'))}
        onKeyDown={(event) => {
          if (event.key === 'ArrowDown') {
            event.preventDefault()
            openAt('first')
          } else if (event.key === 'ArrowUp') {
            event.preventDefault()
            openAt('last')
          }
        }}
      >
        {label}
        <span className="menu__caret" aria-hidden="true">▾</span>
      </button>
      {open && (
        <div
          className="menu__list"
          role="menu"
          id={`${id}-list`}
          aria-labelledby={`${id}-button`}
          onKeyDown={(event) => {
            switch (event.key) {
              case 'ArrowDown':
                event.preventDefault()
                step(1)
                break
              case 'ArrowUp':
                event.preventDefault()
                step(-1)
                break
              case 'Home':
                event.preventDefault()
                setFocusIndex(usable[0])
                break
              case 'End':
                event.preventDefault()
                setFocusIndex(usable[usable.length - 1])
                break
              case 'Escape':
                event.preventDefault()
                close(true)
                break
              case 'Tab':
                close(false)
                break
            }
          }}
        >
          {items.map((item, index) => (
            <button
              key={item.key}
              ref={(el) => {
                itemRefs.current[index] = el
              }}
              type="button"
              role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
              aria-checked={item.checked === undefined ? undefined : item.checked}
              tabIndex={index === focusIndex ? 0 : -1}
              disabled={item.disabled}
              className="menu__item"
              onClick={() => {
                close(true)
                item.onSelect()
              }}
            >
              <span className="menu__check" aria-hidden="true">{item.checked ? '✓' : ''}</span>
              <span className="menu__label">{item.label}</span>
              {item.hint && <span className="menu__hint">{item.hint}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
