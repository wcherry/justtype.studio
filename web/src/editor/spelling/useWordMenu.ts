import { useEffect, useState } from 'react'
import type { Editor } from '@tiptap/react'
import { api } from '../../api/client'
import type { MenuTarget, SelectionAction } from '../../components/SelectionMenu'
import { matchCase } from '../../components/ThesaurusPanel'
import { misspellingAt } from './spellcheck'
import type { useSpelling } from './useSpelling'

export interface WordAt {
  from: number
  to: number
  text: string
}

/** Puts a word in place of another, if the other is still where it was. */
export function replaceWord(editor: Editor | null, at: WordAt, replacement: string): WordAt | string | null {
  if (!editor) return null
  const { doc } = editor.state
  if (at.to > doc.content.size || doc.textBetween(at.from, at.to) !== at.text) {
    return `“${at.text}” is not where it was. Right-click it again.`
  }
  const word = matchCase(replacement, at.text)
  editor.view.dispatch(editor.state.tr.insertText(word, at.from, at.to))
  return { from: at.from, to: at.from + word.length, text: word }
}

/**
 * The spelling and thesaurus part of the right-click menu, for the word it
 * was opened on: corrections if it is misspelled, adding it to the writer's
 * dictionary or ignoring it, and a few synonyms. Shared by every editor that
 * checks spelling, so a word is handled the same way wherever it is typed.
 */
export function useWordMenu(
  editor: Editor | null,
  target: MenuTarget | null,
  {
    spelling,
    spellcheck,
    writable,
    onError,
    onThesaurus,
  }: {
    spelling: ReturnType<typeof useSpelling>
    spellcheck: boolean
    writable: boolean
    onError: (message: string) => void
    /** Opens the thesaurus panel on the word, where the page has one. */
    onThesaurus?: (at: WordAt) => void
  },
): () => SelectionAction[] {
  /** `null` suggestions or synonyms while they are being found. */
  const [menuWords, setMenuWords] = useState<{
    at: WordAt
    misspelled: boolean
    suggestions: string[] | null
    synonyms: string[] | null
  } | null>(null)

  useEffect(() => {
    if (!target || !editor) return setMenuWords(null)
    const selected = target.selection && target.selection.quote.trim()
    const at = target.word
      ?? (selected && !/\s/.test(selected) ? { from: target.selection!.from, to: target.selection!.to, text: target.selection!.quote } : null)
    if (!at) return setMenuWords(null)
    const misspelled = !!misspellingAt(editor.state, at.from) && spellcheck
    let stale = false
    setMenuWords({ at, misspelled, suggestions: misspelled ? null : [], synonyms: null })
    if (misspelled) {
      void spelling.suggest(at.text).then((suggestions) => {
        if (!stale) setMenuWords((m) => (m && m.at === at ? { ...m, suggestions } : m))
      })
    }
    api.words.thesaurus(at.text).then(
      (entry) => {
        const words = entry.senses.slice(0, 3).flatMap((s) => [...s.synonyms, ...s.related])
        const synonyms = [...new Set(words.filter((w) => !w.includes(' ')))].slice(0, 5)
        if (!stale) setMenuWords((m) => (m && m.at === at ? { ...m, synonyms } : m))
      },
      () => {
        if (!stale) setMenuWords((m) => (m && m.at === at ? { ...m, synonyms: [] } : m))
      },
    )
    return () => {
      stale = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, editor])

  return () => {
    if (!menuWords) return []
    const { at, misspelled, suggestions, synonyms } = menuWords
    const actions: SelectionAction[] = []
    const replace = (word: string) => () => {
      const result = replaceWord(editor, at, word)
      if (typeof result === 'string') onError(result)
    }
    if (misspelled) {
      if (suggestions === null) {
        actions.push({ key: 'spell-wait', label: 'Looking for corrections…', disabled: true })
      } else if (suggestions.length === 0) {
        actions.push({ key: 'spell-none', label: 'No suggestions', disabled: true })
      } else if (writable) {
        suggestions.slice(0, 5).forEach((s) => actions.push({ key: `spell-${s}`, label: s, onSelect: replace(s) }))
      }
      actions.push({ key: 'spell-add', label: `Add “${at.text}” to dictionary`, divider: true, onSelect: () => void spelling.add(at.text) })
      actions.push({ key: 'spell-ignore', label: 'Ignore', onSelect: () => spelling.ignore(at.text) })
    }
    if (writable && synonyms && synonyms.length > 0) {
      synonyms.forEach((s, i) => actions.push({
        key: `syn-${s}`, label: s, onSelect: replace(s),
        ...(i === 0 ? { heading: 'Synonyms', divider: actions.length > 0 } : {}),
      }))
    }
    if (onThesaurus) {
      actions.push({
        key: 'thesaurus',
        label: 'Thesaurus…',
        hint: '⇧F7',
        divider: actions.length > 0,
        onSelect: () => onThesaurus(at),
      })
    }
    return actions
  }
}
