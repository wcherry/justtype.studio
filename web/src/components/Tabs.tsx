import { useId, useRef, type ReactNode } from 'react'

export interface Tab<K extends string> {
  key: K
  label: string
}

/**
 * A row of tabs over one panel. Keyboard as tabs should be: ← and → move
 * between them (and choose), Home and End go to the ends, and Tab goes on into
 * the panel.
 */
export function Tabs<K extends string>({
  label, tabs, active, onChange, children,
}: {
  /** What the set of tabs is, for a screen reader. */
  label: string
  tabs: Tab<K>[]
  active: K
  onChange: (key: K) => void
  /** The active tab's panel. */
  children: ReactNode
}) {
  const id = useId()
  const refs = useRef<(HTMLButtonElement | null)[]>([])
  const at = tabs.findIndex((t) => t.key === active)

  const choose = (index: number) => {
    const next = (index + tabs.length) % tabs.length
    onChange(tabs[next].key)
    refs.current[next]?.focus()
  }

  return (
    <div className="tabs">
      <div
        className="tabs__list"
        role="tablist"
        aria-label={label}
        onKeyDown={(event) => {
          const move = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: tabs.length - 1 }[event.key]
          if (move === undefined) return
          event.preventDefault()
          choose(move)
        }}
      >
        {tabs.map((tab, index) => (
          <button
            key={tab.key}
            ref={(el) => {
              refs.current[index] = el
            }}
            type="button"
            role="tab"
            id={`${id}-${tab.key}`}
            aria-selected={tab.key === active}
            aria-controls={`${id}-panel`}
            tabIndex={tab.key === active ? 0 : -1}
            className={tab.key === active ? 'tabs__tab is-active' : 'tabs__tab'}
            onClick={() => onChange(tab.key)}
          >
            {tab.label}
          </button>
        ))}
      </div>
      <div className="tabs__panel" role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-${active}`}>
        {children}
      </div>
    </div>
  )
}
