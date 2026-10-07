import { useEffect, useId, useRef, useState, type ReactNode } from 'react'

export interface MenuItem {
  key: string
  label: string
  /** Shown on the right, e.g. a shortcut. */
  hint?: string
  /** Marks the item whose panel is open, or the option in force. */
  checked?: boolean
  disabled?: boolean
  /** Draws a rule above the item, to set off a group. */
  divider?: boolean
  onSelect?: () => void
  /** A submenu instead of an action. */
  items?: MenuItem[]
  /** A label over the items after it, not something to choose. */
  heading?: boolean
}

/**
 * A button that opens a list of actions, which may open lists of their own.
 *
 * Keyboard as a menu should be: Enter, Space or ↓ opens it on the first item,
 * ↑ on the last; the arrows, Home and End move; → (or Enter) opens a submenu
 * and ← or Escape goes back out of one; Escape at the top, or Tab, closes it
 * all and focus goes back to the button. A click anywhere else closes it too.
 * With the pointer, resting on an item with a submenu opens it.
 */
export function Menu({
  label, items, active = false, title, className, align = 'right', ariaLabel, tip, caret = true,
}: {
  label: ReactNode
  items: MenuItem[]
  /** One of its items is open, so the button shows it. */
  active?: boolean
  title?: string
  className?: string
  /** Which edge of the button the list lines up with. */
  align?: 'left' | 'right'
  /** The button's name, for when its label is only an icon. */
  ariaLabel?: string
  /** A help tip under the button, as the toolbar's icon buttons have. */
  tip?: string
  /** The ▾ after the label; an icon that already reads as a menu goes without. */
  caret?: boolean
}) {
  const [open, setOpen] = useState<'first' | 'last' | null>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const id = useId()

  useEffect(() => {
    if (!open) return
    const away = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(null)
    }
    document.addEventListener('pointerdown', away)
    return () => document.removeEventListener('pointerdown', away)
  }, [open])

  const openAt = (which: 'first' | 'last') => {
    if (items.every((item) => item.disabled || item.heading)) return
    setOpen(which)
  }

  const close = (refocus: boolean) => {
    setOpen(null)
    if (refocus) buttonRef.current?.focus()
  }

  const classes = [
    'menu__button', className, active && 'is-active', tip && 'has-tip', tip && align === 'left' && 'tip-start',
  ].filter(Boolean).join(' ')

  return (
    <div className={align === 'left' ? 'menu menu--left' : 'menu'} ref={rootRef}>
      <button
        ref={buttonRef}
        type="button"
        id={`${id}-button`}
        className={classes}
        aria-haspopup="menu"
        aria-expanded={!!open}
        aria-controls={open ? `${id}-list` : undefined}
        aria-label={ariaLabel}
        data-tip={tip}
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
        {caret && <span className="menu__caret" aria-hidden="true">▾</span>}
      </button>
      {open && (
        <MenuList
          id={`${id}-list`}
          labelledBy={`${id}-button`}
          className="menu__list"
          items={items}
          initial={open}
          onClose={close}
        />
      )}
    </div>
  )
}

function MenuList({
  id, labelledBy, className, items, initial, onClose, onBack,
}: {
  id: string
  labelledBy: string
  className: string
  items: MenuItem[]
  /** Where focus starts; null when the pointer opened it and focus stays put. */
  initial: 'first' | 'last' | null
  onClose: (refocus: boolean) => void
  /** Present on a submenu: back out to the item that opened it. */
  onBack?: () => void
}) {
  const usable = items.map((item, index) => (item.disabled || item.heading ? -1 : index)).filter((i) => i >= 0)
  const [focusIndex, setFocusIndex] = useState(() =>
    initial === 'first' ? usable[0] : initial === 'last' ? usable[usable.length - 1] : -1,
  )
  const [sub, setSub] = useState<{ index: number; initial: 'first' | null } | null>(null)
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([])

  // Focus follows focusIndex, and comes back here when a submenu closes.
  useEffect(() => {
    if (focusIndex >= 0 && (sub === null || sub.initial === null)) itemRefs.current[focusIndex]?.focus()
  }, [focusIndex, sub])

  const step = (by: number) => {
    const at = usable.indexOf(focusIndex)
    const next = at < 0 ? (by > 0 ? usable[0] : usable[usable.length - 1]) : usable[(at + by + usable.length) % usable.length]
    setSub(null)
    setFocusIndex(next)
  }

  const enter = (index: number) => {
    setFocusIndex(index)
    setSub({ index, initial: 'first' })
  }

  return (
    <div
      className={className}
      role="menu"
      id={id}
      aria-labelledby={labelledBy}
      onKeyDown={(event) => {
        // A submenu sits inside its parent's list; each list answers only for
        // its own items.
        if ((event.target as HTMLElement).closest('[role="menu"]') !== event.currentTarget) return
        const item = items[focusIndex]
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
            setSub(null)
            setFocusIndex(usable[0])
            break
          case 'End':
            event.preventDefault()
            setSub(null)
            setFocusIndex(usable[usable.length - 1])
            break
          case 'ArrowRight':
            if (item?.items && !item.disabled) {
              event.preventDefault()
              enter(focusIndex)
            }
            break
          case 'ArrowLeft':
            if (onBack) {
              event.preventDefault()
              onBack()
            }
            break
          case 'Escape':
            event.preventDefault()
            // A submenu the pointer opened goes first, one level at a time.
            if (sub) setSub(null)
            else if (onBack) onBack()
            else onClose(true)
            break
          case 'Tab':
            onClose(false)
            break
        }
      }}
    >
      {items.map((item, index) => {
        if (item.heading) {
          return (
            <div key={item.key} className="menu__heading" role="presentation">
              {item.label}
            </div>
          )
        }
        const opensSub = !!item.items
        const isOpen = sub?.index === index
        return (
          <div key={item.key} className={item.divider ? 'menu__entry has-divider' : 'menu__entry'} role="none">
            <button
              ref={(el) => {
                itemRefs.current[index] = el
              }}
              type="button"
              id={`${id}-${item.key}`}
              role={item.checked === undefined || opensSub ? 'menuitem' : 'menuitemcheckbox'}
              aria-checked={item.checked === undefined || opensSub ? undefined : item.checked}
              aria-haspopup={opensSub ? 'menu' : undefined}
              aria-expanded={opensSub ? isOpen : undefined}
              tabIndex={index === focusIndex ? 0 : -1}
              disabled={item.disabled}
              className={isOpen ? 'menu__item is-open' : 'menu__item'}
              onPointerEnter={() => {
                if (item.disabled) return
                setFocusIndex(index)
                setSub(opensSub ? { index, initial: null } : null)
              }}
              onClick={() => {
                if (opensSub) {
                  enter(index)
                  return
                }
                onClose(true)
                item.onSelect?.()
              }}
            >
              <span className="menu__check" aria-hidden="true">{item.checked ? '✓' : ''}</span>
              <span className="menu__label">{item.label}</span>
              {opensSub ? (
                <span className="menu__hint" aria-hidden="true">▸</span>
              ) : (
                item.hint && <span className="menu__hint">{item.hint}</span>
              )}
            </button>
            {isOpen && item.items && (
              <MenuList
                // Opened by the pointer and then entered by key: start afresh
                // with focus on its first item.
                key={sub.initial ?? 'pointer'}
                id={`${id}-${item.key}-list`}
                labelledBy={`${id}-${item.key}`}
                className="menu__list menu__sub"
                items={item.items}
                initial={sub.initial}
                onClose={onClose}
                onBack={() => setSub(null)}
              />
            )}
          </div>
        )
      })}
    </div>
  )
}
