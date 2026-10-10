import { Extension } from '@tiptap/core'
import type { EditorState, Transaction } from '@tiptap/pm/state'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import type { Node as PMNode } from '@tiptap/pm/model'
import { Decoration, DecorationSet } from '@tiptap/pm/view'
import type { Speller } from './speller'

interface SpellState {
  decorations: DecorationSet
  /** The word left unmarked because it is still being typed. */
  typing: { from: number; to: number } | null
}

export const spellcheckKey = new PluginKey<SpellState>('spellcheck')

/**
 * Words, as a dictionary sees them: letters, with apostrophes inside
 * ("don't", "o'er"). Hyphens split, so "half-elven" is checked as two words.
 */
const WORD = /[\p{L}\p{M}\p{N}]+(?:['’][\p{L}\p{M}]+)*/gu
/** A non-text leaf (a line break) is one position, so it is one character. */
const LEAF = '￼'

/** Not worth a dictionary's opinion: numbers, initials, and words like "iPhone" or "McAllister". */
function skip(word: string) {
  if (word.length < 2 || /\p{N}/u.test(word)) return true
  const rest = word.slice(1)
  return rest !== rest.toLowerCase() && word !== word.toUpperCase()
}

/** Every word in a text block, with where it sits in the document. */
export function wordsIn(block: PMNode, start: number) {
  const text = block.textBetween(0, block.content.size, undefined, LEAF)
  const out: { from: number; to: number; text: string }[] = []
  for (const match of text.matchAll(WORD)) {
    const from = start + 1 + match.index!
    out.push({ from, to: from + match[0].length, text: match[0] })
  }
  return out
}

/** The word around a position, if there is one. */
export function wordAt(state: EditorState, pos: number) {
  const $pos = state.doc.resolve(pos)
  const block = $pos.parent
  if (!block.isTextblock) return null
  const start = $pos.before()
  return wordsIn(block, start).find((w) => w.from <= pos && pos <= w.to) ?? null
}

/** The misspelled word at a position, if the checker has marked one there. */
export function misspellingAt(state: EditorState, pos: number) {
  const found = spellcheckKey.getState(state)?.decorations.find(pos, pos)
  const deco = found?.find((d) => d.from <= pos && pos <= d.to)
  return deco ? { from: deco.from, to: deco.to, text: state.doc.textBetween(deco.from, deco.to) } : null
}

/**
 * Underlines misspelled words, with this app's own spellchecker rather than
 * the browser's: one dictionary everywhere, that knows the names in the
 * writer's notebook and the words they have added, and offers its corrections
 * in the editor's own menu.
 *
 * Decorations, never marks — an underline is an opinion about the text, not
 * part of it, and must not reach the saved document. After an edit only the
 * blocks it touched are checked again; the word under the caret is left alone
 * until the writer has finished typing it.
 */
export const Spellcheck = Extension.create<{ speller: Speller | null; enabled: boolean }>({
  name: 'spellcheck',

  addOptions() {
    return { speller: null, enabled: true }
  },

  addProseMirrorPlugins() {
    const extension = this
    const active = () => (extension.options.enabled ? extension.options.speller : null)

    /** Decorations for one block, leaving out the word still being typed. */
    const check = (state: EditorState, block: PMNode, pos: number, speller: Speller, caret: number | null) => {
      const decorations: Decoration[] = []
      let typing: SpellState['typing'] = null
      const code = state.schema.marks.code
      for (const word of wordsIn(block, pos)) {
        if (skip(word.text)) continue
        if (caret === word.to) {
          typing = { from: word.from, to: word.to }
          continue
        }
        if (speller.check(word.text) !== false) continue
        if (code && state.doc.rangeHasMark(word.from, word.to, code)) continue
        decorations.push(Decoration.inline(word.from, word.to, { class: 'spell-error' }))
      }
      return { decorations, typing }
    }

    const caretOf = (state: EditorState) => (state.selection.empty ? state.selection.from : null)

    const everything = (state: EditorState): SpellState => {
      const speller = active()
      if (!speller) return { decorations: DecorationSet.empty, typing: null }
      const all: Decoration[] = []
      let typing: SpellState['typing'] = null
      state.doc.descendants((node, pos) => {
        if (!node.isTextblock) return true
        const result = check(state, node, pos, speller, caretOf(state))
        all.push(...result.decorations)
        typing = result.typing ?? typing
        return false
      })
      return { decorations: DecorationSet.create(state.doc, all), typing }
    }

    /** Re-checks just the blocks touching the given ranges. */
    const some = (old: SpellState, tr: Transaction, state: EditorState, ranges: { from: number; to: number }[]): SpellState => {
      const speller = active()
      if (!speller) return { decorations: DecorationSet.empty, typing: null }
      let decorations = old.decorations.map(tr.mapping, tr.doc)
      let typing = old.typing
      const size = state.doc.content.size
      const seen = new Set<number>()
      for (const range of ranges) {
        const from = Math.max(0, Math.min(range.from, size))
        const to = Math.max(from, Math.min(range.to, size))
        state.doc.nodesBetween(Math.max(0, from - 1), Math.min(size, to + 1), (node, pos) => {
          if (!node.isTextblock) return true
          if (seen.has(pos)) return false
          seen.add(pos)
          decorations = decorations.remove(decorations.find(pos, pos + node.nodeSize))
          const result = check(state, node, pos, speller, caretOf(state))
          decorations = decorations.add(state.doc, result.decorations)
          if (result.typing) typing = result.typing
          else if (typing && typing.from >= pos && typing.to <= pos + node.nodeSize) typing = null
          return false
        })
      }
      return { decorations, typing }
    }

    return [
      new Plugin<SpellState>({
        key: spellcheckKey,
        state: {
          init: (_, state) => everything(state),
          apply(tr, old, _oldState, state) {
            if (tr.getMeta(spellcheckKey) === 'refresh') return everything(state)
            if (!active()) return old.decorations === DecorationSet.empty ? old : { decorations: DecorationSet.empty, typing: null }
            const ranges: { from: number; to: number }[] = []
            if (tr.docChanged) {
              tr.mapping.maps.forEach((map, i) => {
                const rest = tr.mapping.slice(i + 1)
                map.forEach((_a, _b, from, to) => ranges.push({ from: rest.map(from, -1), to: rest.map(to, 1) }))
              })
            }
            // The caret has left a word it skipped: that word is finished now.
            const typing = old.typing && {
              from: tr.mapping.map(old.typing.from, -1),
              to: tr.mapping.map(old.typing.to, 1),
            }
            if (typing && caretOf(state) !== typing.to) ranges.push(typing)
            if (ranges.length === 0) return tr.docChanged ? { ...old, decorations: old.decorations.map(tr.mapping, tr.doc) } : old
            return some({ ...old, typing }, tr, state, ranges)
          },
        },
        props: {
          decorations: (state) => spellcheckKey.getState(state)?.decorations ?? DecorationSet.empty,
        },
      }),
    ]
  },
})

/** Asks the editor to look at every word again: new words taught, or the checker switched. */
export function refreshSpelling(state: EditorState) {
  return state.tr.setMeta(spellcheckKey, 'refresh').setMeta('addToHistory', false)
}
