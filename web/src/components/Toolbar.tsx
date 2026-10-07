import type { ReactNode } from 'react'
import type { Editor } from '@tiptap/react'

import { Icon } from './Icon'
import type { IconName } from './Icon'

interface Props {
  editor: Editor | null
  /** Formatting is offered only where the writer can change the text. */
  canFormat: boolean
  /** The everyday actions, at the right-hand end. */
  children?: ReactNode
}

export interface FormatAction {
  label: string
  title: string
  isActive: (e: Editor) => boolean
  run: (e: Editor) => void
}

/** Deliberately small: the formatting a manuscript actually needs, nothing else. */
export const FORMAT_ACTIONS: FormatAction[] = [
  { label: 'B', title: 'Bold (⌘B)', isActive: (e) => e.isActive('bold'), run: (e) => e.chain().focus().toggleBold().run() },
  { label: 'I', title: 'Italic (⌘I)', isActive: (e) => e.isActive('italic'), run: (e) => e.chain().focus().toggleItalic().run() },
  { label: 'U', title: 'Underline (⌘U)', isActive: (e) => e.isActive('underline'), run: (e) => e.chain().focus().toggleUnderline().run() },
  { label: 'S', title: 'Strikethrough', isActive: (e) => e.isActive('strike'), run: (e) => e.chain().focus().toggleStrike().run() },
  { label: 'H2', title: 'Section heading', isActive: (e) => e.isActive('heading', { level: 2 }), run: (e) => e.chain().focus().toggleHeading({ level: 2 }).run() },
  { label: '“”', title: 'Block quote', isActive: (e) => e.isActive('blockquote'), run: (e) => e.chain().focus().toggleBlockquote().run() },
  { label: '•', title: 'Bulleted list', isActive: (e) => e.isActive('bulletList'), run: (e) => e.chain().focus().toggleBulletList().run() },
  { label: '1.', title: 'Numbered list', isActive: (e) => e.isActive('orderedList'), run: (e) => e.chain().focus().toggleOrderedList().run() },
  { label: '—', title: 'Scene break', isActive: () => false, run: (e) => e.chain().focus().setHorizontalRule().run() },
]

export function Toolbar({ editor, canFormat, children }: Props) {
  const formatting = canFormat && editor
  return (
    <div className="toolbar" role="toolbar" aria-label="Writing">
      {formatting && FORMAT_ACTIONS.map((action) => (
        <button
          key={action.label}
          type="button"
          title={action.title}
          aria-label={action.title}
          className={action.isActive(editor) ? 'toolbar__btn is-active' : 'toolbar__btn'}
          onMouseDown={(event) => {
            // Keep the selection: the editor must not lose focus to the button.
            event.preventDefault()
            action.run(editor)
          }}
        >
          {action.label}
        </button>
      ))}
      {formatting && (
        <>
          <span className="toolbar__divider" aria-hidden="true" />
          <button
            type="button"
            className="toolbar__btn"
            title="Undo (⌘Z)"
            aria-label="Undo"
            onMouseDown={(e) => { e.preventDefault(); editor.chain().focus().undo().run() }}
          >
            <Icon name="undo" />
          </button>
          <button
            type="button"
            className="toolbar__btn"
            title="Redo (⇧⌘Z)"
            aria-label="Redo"
            onMouseDown={(e) => { e.preventDefault(); editor.chain().focus().redo().run() }}
          >
            <Icon name="redo" />
          </button>
        </>
      )}
      <span className="toolbar__spacer" />
      {children}
    </div>
  )
}

/**
 * An icon button with a help tip: shown on hover or keyboard focus, under the
 * button, and read as the button's name.
 */
export function ToolButton({
  icon, label, pressed, onClick, tipAlign = 'end', expanded, controls, disabled,
}: {
  icon: IconName
  label: string
  /** Set for a toggle: whether it is on. */
  pressed?: boolean
  onClick: () => void
  /**
   * Where the tip lines up. These buttons sit at the right-hand end of their
   * bars, so by default it lines up with the button's end and stays on screen.
   */
  tipAlign?: 'center' | 'end'
  /** Set for a button that opens a popup: whether it is open. */
  expanded?: boolean
  controls?: string
  disabled?: boolean
}) {
  const classes = ['toolbar__btn', 'has-tip', tipAlign === 'end' && 'tip-end', pressed && 'is-active']
  return (
    <button
      type="button"
      className={classes.filter(Boolean).join(' ')}
      data-tip={label}
      aria-label={label}
      aria-pressed={expanded === undefined ? pressed : undefined}
      aria-expanded={expanded}
      aria-controls={controls}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} />
    </button>
  )
}
