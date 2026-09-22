import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'

const key = new PluginKey<DecorationSet>('focusMode')

/**
 * Dims every block except the one the caret is in.
 *
 * A decoration rather than a mark, for the same reason comment anchors are: the
 * author's document must not carry a record of where their cursor happened to
 * be. Everything here is a class on a view-layer overlay, so turning focus mode
 * off leaves no trace in the saved JSON.
 *
 * Block-level rather than sentence-level. Sentence splitting is a guess in any
 * language and a bad one in dialogue, where a line can end mid-quote.
 */
export const FocusMode = Extension.create<{ enabled: boolean }>({
  name: 'focusMode',

  addOptions() {
    return { enabled: false }
  },

  addProseMirrorPlugins() {
    const extension = this
    return [
      new Plugin({
        key,
        props: {
          decorations(state) {
            if (!extension.options.enabled) return DecorationSet.empty

            // The depth-1 ancestor of the caret is the block to keep lit.
            const { $from } = state.selection
            const activeStart = $from.depth > 0 ? $from.before(1) : -1

            const decorations: Decoration[] = []
            state.doc.forEach((node, offset) => {
              if (offset !== activeStart) {
                decorations.push(
                  Decoration.node(offset, offset + node.nodeSize, { class: 'is-dimmed' }),
                )
              }
            })
            return DecorationSet.create(state.doc, decorations)
          },
        },
      }),
    ]
  },
})
