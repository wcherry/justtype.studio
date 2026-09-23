import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'

export type ReactionKind = 'loved' | 'confusing' | 'dragging'

/**
 * The glyph, the sentence a reader is saying, and the word an author reads it
 * back as — in the order the API declares them, so a tally never reshuffles.
 */
export const REACTIONS: { kind: ReactionKind; glyph: string; label: string; noun: string }[] = [
  { kind: 'loved', glyph: '♥', label: 'This landed', noun: 'loved it' },
  { kind: 'confusing', glyph: '?', label: 'I got lost here', noun: 'got lost' },
  { kind: 'dragging', glyph: '~', label: 'I was skimming by now', noun: 'skimmed' },
]

interface Options {
  /** What this reader marked, by block index. */
  marks: Record<number, ReactionKind>
  onReact: (blockIndex: number, kind: ReactionKind) => void
}

const key = new PluginKey<DecorationSet>('reactionGutter')

/**
 * A one-tap reaction beside every paragraph.
 *
 * The buttons are widget decorations, so they live in the view and never in
 * the document — the same reason comment highlights are decorations. They are
 * absolutely positioned out into the margin, which is why the stylesheet makes
 * every block in a reader a positioning context.
 *
 * Reactions are per *block*, not per selection: two readers never highlight
 * quite the same words, and a tally nobody can add up is not a signal.
 */
export const ReactionGutter = Extension.create<Options>({
  name: 'reactionGutter',

  addOptions() {
    return { marks: {}, onReact: () => {} }
  },

  addProseMirrorPlugins() {
    const extension = this
    return [
      new Plugin({
        key,
        props: {
          decorations(state) {
            const decorations: Decoration[] = []
            state.doc.forEach((node, offset, index) => {
              if (node.isText || node.type.name === 'horizontalRule') return
              decorations.push(
                Decoration.widget(offset + 1, () => gutter(extension.options, index), {
                  side: -1,
                  // Without this the widget counts as a click target inside
                  // the document and a tap on a button moves the selection,
                  // which fires the highlight-to-comment flow instead.
                  ignoreSelection: true,
                }),
              )
            })
            return DecorationSet.create(state.doc, decorations)
          },
        },
      }),
    ]
  },
})

function gutter(options: Options, blockIndex: number): HTMLElement {
  const mine = options.marks[blockIndex]
  const wrap = document.createElement('span')
  wrap.className = mine ? 'reactions reactions--set' : 'reactions'
  wrap.contentEditable = 'false'

  for (const { kind, glyph, label } of REACTIONS) {
    const button = document.createElement('button')
    button.type = 'button'
    button.className = kind === mine ? 'reactions__pick is-mine' : 'reactions__pick'
    button.textContent = glyph
    button.title = kind === mine ? `${label} — tap again to take it back` : label
    button.addEventListener('mousedown', (event) => {
      // The reader is read-only, but a mousedown inside it still collapses the
      // selection; the comment box would blink out from under the reader.
      event.preventDefault()
      event.stopPropagation()
    })
    button.addEventListener('click', (event) => {
      event.preventDefault()
      event.stopPropagation()
      options.onReact(blockIndex, kind)
    })
    wrap.appendChild(button)
  }
  return wrap
}
