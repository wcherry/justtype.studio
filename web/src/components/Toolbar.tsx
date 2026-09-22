import type { Editor } from '@tiptap/react'

interface Props {
  editor: Editor | null
}

interface Action {
  label: string
  title: string
  isActive: (e: Editor) => boolean
  run: (e: Editor) => void
}

/** Deliberately small: the formatting a manuscript actually needs, nothing else. */
const ACTIONS: Action[] = [
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

export function Toolbar({ editor }: Props) {
  if (!editor) return <div className="toolbar" />
  return (
    <div className="toolbar">
      {ACTIONS.map((action) => (
        <button
          key={action.label}
          type="button"
          title={action.title}
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
      <span className="toolbar__spacer" />
      <button
        type="button"
        className="toolbar__btn"
        title="Undo (⌘Z)"
        onMouseDown={(e) => { e.preventDefault(); editor.chain().focus().undo().run() }}
      >
        ↶
      </button>
      <button
        type="button"
        className="toolbar__btn"
        title="Redo (⇧⌘Z)"
        onMouseDown={(e) => { e.preventDefault(); editor.chain().focus().redo().run() }}
      >
        ↷
      </button>
    </div>
  )
}
