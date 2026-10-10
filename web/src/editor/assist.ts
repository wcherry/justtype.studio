import type { Editor } from '@tiptap/react'

/** Where a suggestion goes, and what was there when it was asked for. */
export interface AssistRange {
  from: number
  to: number
  /** The text in the range at the time, to tell whether it has since changed. */
  original: string
}

/** What the assistant is handed: the passage, and the scene either side of it. */
export function snapshot(editor: Editor) {
  const { doc, selection } = editor.state
  const { from, to } = selection
  const text = (a: number, b: number) => doc.textBetween(a, b, '\n\n', ' ')
  return {
    range: { from, to, original: text(from, to) } as AssistRange,
    before: text(0, from),
    selection: text(from, to),
    after: text(to, doc.content.size),
  }
}

/** The assistant's text as paragraphs. */
function paragraphs(text: string) {
  return text
    .split(/\n\s*\n/)
    .map((p) => p.replace(/\s*\n\s*/g, ' ').trim())
    .filter(Boolean)
}

/**
 * Puts a suggestion into the scene in place of its range, as one step the
 * writer can undo. The first paragraph runs on from the text before it; any
 * more become paragraphs of their own.
 *
 * Refused, with a reason, when the words in the range are not the ones the
 * suggestion was written for — the writer has carried on typing, and a
 * rewrite of words that are no longer there would land on the wrong ones.
 */
export function applySuggestion(
  editor: Editor,
  range: AssistRange,
  text: string,
  { continuing = false }: { continuing?: boolean } = {},
): string | null {
  const { state } = editor
  if (range.to > state.doc.content.size || state.doc.textBetween(range.from, range.to, '\n\n', ' ') !== range.original) {
    return 'The text has changed since this was written. Select the passage again and ask again.'
  }
  const parts = paragraphs(text)
  if (parts.length === 0) return 'There was nothing to put in.'

  const tr = state.tr
  let at = range.from
  const $at = state.doc.resolve(range.from)
  const before = $at.parent.textBetween(0, $at.parentOffset)
  if (continuing && before.trim()) {
    // At the end of a finished sentence that ends its paragraph, what comes
    // next starts a paragraph of its own; mid-sentence, it runs on.
    const atEnd = $at.parentOffset === $at.parent.content.size
    if (atEnd && /[.!?…"”’)\]]\s*$/.test(before)) {
      tr.split(at)
      at += 2
    } else if (!/\s$/.test(before) && !/^[,.;:!?…'’”)\]—-]/.test(parts[0])) {
      parts[0] = ' ' + parts[0]
    }
  }
  tr.insertText(parts[0], at, continuing ? at : range.to)
  at += parts[0].length
  for (const part of parts.slice(1)) {
    tr.split(at)
    at += 2
    tr.insertText(part, at)
    at += part.length
  }
  editor.view.dispatch(tr.scrollIntoView())
  editor.view.focus()
  return null
}

/**
 * What changed between two versions of a passage, word by word, for showing
 * a proofread: kept, added and taken out, in order. Too long to compare
 * cheaply, and it is shown whole instead.
 */
export function wordDiff(a: string, b: string): { kind: 'same' | 'add' | 'del'; text: string }[] | null {
  const x = a.split(/(\s+)/)
  const y = b.split(/(\s+)/)
  if (x.length * y.length > 4_000_000) return null
  const n = x.length
  const m = y.length
  // Longest common subsequence, by suffix.
  const table = Array.from({ length: n + 1 }, () => new Uint32Array(m + 1))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      table[i][j] = x[i] === y[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1])
    }
  }
  const out: { kind: 'same' | 'add' | 'del'; text: string }[] = []
  const push = (kind: 'same' | 'add' | 'del', text: string) => {
    const last = out[out.length - 1]
    if (last && last.kind === kind) last.text += text
    else out.push({ kind, text })
  }
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      push('same', x[i])
      i++
      j++
    } else if (table[i + 1][j] >= table[i][j + 1]) {
      push('del', x[i++])
    } else {
      push('add', y[j++])
    }
  }
  while (i < n) push('del', x[i++])
  while (j < m) push('add', y[j++])
  return out
}
