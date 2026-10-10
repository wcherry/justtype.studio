import { useCallback, useEffect, useState } from 'react'
import type { Editor } from '@tiptap/react'
import { api } from '../../api/client'
import type { SpellingLanguage } from './protocol'
import { Speller, normalise } from './speller'
import { refreshSpelling } from './spellcheck'

const CACHE_KEY = 'justtype.dictionary'

function cachedWords(): string[] {
  try {
    return JSON.parse(localStorage.getItem(CACHE_KEY) ?? '[]') as string[]
  } catch {
    return []
  }
}

function cacheWords(words: string[]) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(words))
  } catch {
    // Blocked site data: the server still has them.
  }
}

/** The single words in a name: "Ysolde Marrow-Kent" → Ysolde, Marrow, Kent. */
function wordsOfNames(names: string[]) {
  return names.flatMap((name) => name.match(/[\p{L}\p{M}]+(?:['’][\p{L}\p{M}]+)*/gu) ?? [])
}

/**
 * Spelling, for the editor: the checker for the chosen language, taught the
 * writer's own dictionary and the names in this book's notebook.
 *
 * The dictionary is kept on the server, so it follows the writer between
 * devices, and on this device too, so it is there offline.
 */
export function useSpelling(
  editor: Editor | null,
  { enabled, language, workId }: { enabled: boolean; language: SpellingLanguage; workId: string },
) {
  const [speller, setSpeller] = useState<Speller | null>(null)
  const [words, setWords] = useState<string[]>(cachedWords)
  const [names, setNames] = useState<string[]>([])
  const [error, setError] = useState<string | null>(null)

  // Only built when wanted: the dictionary is half a megabyte.
  useEffect(() => {
    if (!enabled) return
    const created = new Speller(language)
    setSpeller(created)
    return () => {
      created.dispose()
      setSpeller(null)
    }
  }, [enabled, language])

  useEffect(() => {
    api.words.dictionary().then((list) => {
      setWords(list)
      cacheWords(list)
    }).catch(() => undefined)
  }, [])

  const reloadNames = useCallback(() => {
    if (!workId) return
    api.notebook.list(workId)
      .then((entries) => setNames(wordsOfNames(entries.map((e) => e.name))))
      .catch(() => undefined)
  }, [workId])
  useEffect(reloadNames, [reloadNames])

  useEffect(() => {
    speller?.add([...words, ...names])
  }, [speller, words, names])

  // Hands the checker to the editor, and repaints whenever it learns something.
  useEffect(() => {
    if (!editor) return
    const extension = editor.extensionManager.extensions.find((e) => e.name === 'spellcheck')
    if (!extension) return
    extension.options.speller = speller
    extension.options.enabled = enabled
    const repaint = () => {
      if (!editor.isDestroyed) editor.view.dispatch(refreshSpelling(editor.state))
    }
    repaint()
    return speller?.subscribe(repaint)
  }, [editor, speller, enabled])

  const add = useCallback(async (word: string) => {
    const key = normalise(word)
    speller?.add([key])
    setWords((current) => (current.includes(key) ? current : [...current, key].sort()))
    setError(null)
    try {
      const list = await api.words.addToDictionary(key)
      setWords(list)
      cacheWords(list)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not add that word')
    }
  }, [speller])

  const remove = useCallback(async (word: string) => {
    speller?.remove([word])
    setWords((current) => current.filter((w) => w !== word))
    setError(null)
    try {
      const list = await api.words.removeFromDictionary(word)
      setWords(list)
      cacheWords(list)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not take that word out')
    }
  }, [speller])

  const ignore = useCallback((word: string) => speller?.ignore(word), [speller])
  const suggest = useCallback(async (word: string) => (speller ? speller.suggest(word) : []), [speller])

  return { speller, words, add, remove, ignore, suggest, reloadNames, error }
}
