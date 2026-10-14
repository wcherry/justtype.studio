import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import { findQuote } from './annotations'

export interface AnchorSpec {
  id: string
  from: number
  to: number
  resolved?: boolean
  /**
   * Written on an earlier version than this one: its positions are that
   * version's, so it is placed by `quote` instead, nearest where it was.
   */
  carried?: boolean
  quote?: string
}

/** Where an anchor falls in this document, or null if its passage is gone. */
export function place(doc: Parameters<typeof findQuote>[0], anchor: AnchorSpec): { from: number; to: number } | null {
  if (anchor.carried) return anchor.quote ? findQuote(doc, anchor.quote, anchor.from) : null
  const size = doc.content.size
  return anchor.from >= 0 && anchor.to <= size && anchor.to > anchor.from ? anchor : null
}

const key = new PluginKey<DecorationSet>('commentAnchors')

/**
 * Paints comment anchors as ProseMirror decorations rather than marks, so a
 * reviewer's highlights never touch the author's document. Positions come from
 * an immutable snapshot, so they cannot drift — except for a note carried
 * over from an earlier snapshot, which is found again by the words it quoted.
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
            const decorations = anchors
              .flatMap((a) => {
                const at = place(state.doc, a)
                return at ? [{ ...a, from: at.from, to: at.to }] : []
              })
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
