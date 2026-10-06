import type { ReactNode } from 'react'

/** The frame every editor side panel sits in: a title, a way to put it away. */
export function SidePanel({
  title, onClose, children,
}: {
  title: string
  onClose: () => void
  children: ReactNode
}) {
  return (
    <aside className="versions" aria-label={title}>
      <header className="versions__header">
        <h3>{title}</h3>
        <CloseButton onClose={onClose} />
      </header>
      {children}
    </aside>
  )
}

export function CloseButton({ onClose }: { onClose: () => void }) {
  return (
    <button type="button" className="panel-close" aria-label="Close panel" title="Close" onClick={onClose}>
      ×
    </button>
  )
}
