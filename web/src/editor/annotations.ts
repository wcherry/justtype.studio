import { Extension } from '@tiptap/core'
import type { Node as PmNode } from '@tiptap/pm/model'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import type { EditorState } from '@tiptap/pm/state'
import { Decoration, DecorationSet } from '@tiptap/pm/view'

export interface AnnotationSpec {
  id: string
  kind: 'highlight' | 'bookmark' | 'comment'
  from: number
  to: number
}

const key = new PluginKey<DecorationSet>('annotations')

function build(doc: PmNode, annotations: AnnotationSpec[]): DecorationSet {
  const size = doc.content.size
  return DecorationSet.create(
    doc,
    annotations
      .filter((a) => a.from >= 0 && a.to <= size && a.to > a.from)
      .flatMap((a) => {
        const passage = Decoration.inline(
          a.from,
          a.to,
          { class: `mark-${a.kind}`, 'data-annotation-id': a.id },
          { id: a.id },
        )
        if (a.kind !== 'bookmark') return [passage]
        // A bookmark is a place: a flag where it starts, and a faint tint on
        // the passage so it can still be found and right-clicked.
        const flag = () => {
          const el = document.createElement('span')
          el.className = 'mark-bookmark__flag'
          el.setAttribute('data-annotation-id', a.id)
          el.setAttribute('aria-label', 'Bookmark')
          el.title = 'Bookmark'
          el.contentEditable = 'false'
          return el
        }
        return [passage, Decoration.widget(a.from, flag, { side: -1, key: `flag-${a.id}`, flag: true })]
      }),
  )
}

/**
 * Paints a person's own highlights and bookmarks.
 *
 * Decorations, not marks, so they never become part of the manuscript — the
 * same reason comment anchors are decorations. In the editor they are mapped
 * through every change, so a highlight stays on its words while the writer
 * types around it; `currentRanges` reads where they have got to.
 */
export const Annotations = Extension.create<{ annotations: AnnotationSpec[] }>({
  name: 'annotations',

  addOptions() {
    return { annotations: [] }
  },

  addProseMirrorPlugins() {
    const extension = this
    return [
      new Plugin<DecorationSet>({
        key,
        state: {
          init: (_, state) => build(state.doc, extension.options.annotations),
          apply(tr, old, _, state) {
            // A new list from outside wins; otherwise follow the edit.
            if (tr.getMeta(key)) return build(state.doc, extension.options.annotations)
            return tr.docChanged ? old.map(tr.mapping, tr.doc) : old
          },
        },
        props: {
          decorations: (state) => key.getState(state),
        },
      }),
    ]
  },
})

/** Repaints after the list changed outside the editor. */
export function refreshAnnotations(state: EditorState) {
  return state.tr.setMeta(key, true)
}

/** Where each mark is now, after the edits since it was painted. */
export function currentRanges(state: EditorState): Map<string, { from: number; to: number }> {
  const out = new Map<string, { from: number; to: number }>()
  const set = key.getState(state)
  if (!set) return out
  for (const decoration of set.find()) {
    const spec = decoration.spec as { id: string; flag?: boolean }
    if (!spec.flag) out.set(spec.id, { from: decoration.from, to: decoration.to })
  }
  return out
}

/**
 * Finds a quote in the document again, nearest to where it was.
 *
 * The quote was read with `textBetween(from, to, ' ')`, which puts a space
 * between blocks, so the text searched is built the same way and every
 * character remembers its position.
 */
export function findQuote(doc: PmNode, quote: string, near: number): { from: number; to: number } | null {
  if (!quote) return null
  let text = ''
  const positions: number[] = []
  let first = true
  doc.descendants((node, pos) => {
    if (node.isTextblock) {
      if (!first) {
        text += ' '
        positions.push(pos)
      }
      first = false
      return true
    }
    if (node.isText && node.text) {
      for (let i = 0; i < node.text.length; i++) positions.push(pos + i)
      text += node.text
    }
    return true
  })
  let best: { from: number; to: number } | null = null
  for (let at = text.indexOf(quote); at >= 0; at = text.indexOf(quote, at + 1)) {
    const from = positions[at]
    const to = positions[at + quote.length - 1] + 1
    if (!best || Math.abs(from - near) < Math.abs(best.from - near)) best = { from, to }
  }
  return best
}

/** The longest passage a mark keeps, as the server does. */
export const MAX_MARK_QUOTE = 2000

/**
 * Pulls a range in past blank space at either end, so the quote stored is the
 * words themselves — the server trims, and a range that disagreed with its own
 * quote would be "moved" again on every save.
 */
export function tighten(doc: PmNode, range: { from: number; to: number }): { from: number; to: number } | null {
  let { from, to } = range
  while (from < to && /^\s?$/.test(doc.textBetween(from, from + 1, ' '))) from++
  while (to > from && /^\s?$/.test(doc.textBetween(to - 1, to, ' '))) to--
  return to > from ? { from, to } : null
}
