import { Extension } from '@tiptap/core'

/** Where a double Enter landed, once its blank line has been taken out. */
export interface SceneBreak {
  /** The top-level block that starts what comes after the break. */
  atBlock: number
  /** Nothing came after it: the break was at the end of the scene. */
  atEnd: boolean
}

/**
 * A blank line as a scene break: Enter on an empty paragraph, the second
 * Enter of a double Enter, takes that empty line out and hands the break to
 * the page, which moves on to the next scene (or splits this one there).
 *
 * Only a top-level paragraph counts, so an empty list item still ends the
 * list as Enter always has, and only one after some text, so Enter at the
 * top of a scene is just Enter. Off unless the writer has asked for it.
 */
export const SceneBreakOnBlankLine = Extension.create<{
  enabled: boolean
  onBreak: ((at: SceneBreak) => void) | null
}>({
  name: 'sceneBreak',
  // Ahead of the paragraph and list keymaps, which would otherwise take Enter.
  priority: 1000,

  addOptions() {
    return { enabled: false, onBreak: null }
  },

  addKeyboardShortcuts() {
    return {
      Enter: ({ editor }) => {
        const { enabled, onBreak } = this.options
        if (!enabled || !onBreak || !editor.isEditable) return false
        const { selection, doc } = editor.state
        const { $from, empty } = selection
        if (!empty || $from.depth !== 1) return false
        const block = $from.parent
        if (block.type.name !== 'paragraph' || block.content.size > 0) return false
        const index = $from.index(0)
        // Blank lines at the top of a scene are not a break: nothing to end.
        if (!doc.textBetween(0, $from.before(1), ' ').trim()) return false
        const atEnd = index === doc.childCount - 1
        editor.view.dispatch(editor.state.tr.delete($from.before(1), $from.after(1)))
        onBreak({ atBlock: index, atEnd })
        return true
      },
    }
  },
})
