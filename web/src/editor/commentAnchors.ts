import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'

export interface AnchorSpec {
  id: string
  from: number
  to: number
  resolved?: boolean
}

const key = new PluginKey<DecorationSet>('commentAnchors')

/**
 * Paints comment anchors as ProseMirror decorations rather than marks, so a
 * reviewer's highlights never touch the author's document. Positions come from
 * an immutable snapshot, so they cannot drift.
 */
export const CommentAnchors = Extension.create<{ anchors: AnchorSpec[]; activeId: string | null }>({
  name: 'commentAnchors',

  addOptions() {
    return { anchors: [], activeId: null }
  },

  addProseMirrorPlugins() {
    const extension = this
    return [
      new Plugin({
        key,
        props: {
          decorations(state) {
            const { anchors, activeId } = extension.options
            const size = state.doc.content.size
            const decorations = anchors
              .filter((a) => a.from >= 0 && a.to <= size && a.to > a.from)
              .map((a) =>
                Decoration.inline(a.from, a.to, {
                  class: [
                    'anchor',
                    a.resolved ? 'anchor--resolved' : '',
                    a.id === activeId ? 'anchor--active' : '',
                  ]
                    .filter(Boolean)
                    .join(' '),
                  'data-comment-id': a.id,
                }),
              )
            return DecorationSet.create(state.doc, decorations)
          },
        },
      }),
    ]
  },
})
