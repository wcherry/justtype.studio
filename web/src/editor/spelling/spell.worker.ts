/// <reference lib="webworker" />
/**
 * The spellchecker, off the page's thread.
 *
 * Building the dictionary takes a second or so and suggesting a correction
 * can take tens of milliseconds; neither should land between two keystrokes.
 * The page sends words, and gets back which of them are wrong.
 *
 * Nothing here talks to anything but the dictionary files this app serves:
 * the writer's words stay in their browser.
 */
import nspell from 'nspell'
import type { FromWorker, ToWorker } from './protocol'

let speller: ReturnType<typeof nspell> | null = null
/** Words the writer or the book has vouched for, applied once loaded too. */
const personal = new Set<string>()

const post = (message: FromWorker) => postMessage(message)

/** Right as written, or right once the ways fiction bends a word are undone. */
function correct(word: string): boolean {
  if (!speller) return true
  if (personal.has(word) || speller.correct(word)) return true
  const lower = word.toLowerCase()
  // A capital at the start of a sentence, or a word shouted.
  if (word !== lower && (personal.has(lower) || speller.correct(lower))) return true
  if (word === word.toUpperCase() && word.length > 1) {
    const title = word[0] + lower.slice(1)
    if (personal.has(title) || speller.correct(title)) return true
  }
  // Possessives of names the dictionary will never know: "Ysolde's".
  const possessive = /^(.+?)'s?$/.exec(word)
  if (possessive && possessive[1] !== word && correct(possessive[1])) return true
  return false
}

addEventListener('message', async (event: MessageEvent<ToWorker>) => {
  const message = event.data
  switch (message.type) {
    case 'load': {
      try {
        const base = `/dictionaries/${message.language}`
        const [aff, dic] = await Promise.all(
          ['index.aff', 'index.dic'].map(async (file) => {
            const res = await fetch(`${base}/${file}`)
            if (!res.ok) throw new Error(`${file}: ${res.status}`)
            return res.text()
          }),
        )
        speller = nspell(aff, dic)
        // Added to the speller as well, so a name can be suggested for a
        // misspelling of it.
        personal.forEach((w) => speller!.add(w))
        post({ type: 'ready' })
      } catch (e) {
        post({ type: 'failed', message: e instanceof Error ? e.message : String(e) })
      }
      break
    }
    case 'add':
      for (const word of message.words) {
        personal.add(word)
        speller?.add(word)
      }
      break
    case 'remove':
      for (const word of message.words) {
        personal.delete(word)
        speller?.remove(word)
      }
      break
    case 'check':
      post({ type: 'checked', id: message.id, wrong: message.words.filter((w) => !correct(w)) })
      break
    case 'suggest':
      post({
        type: 'suggested',
        id: message.id,
        suggestions: speller ? speller.suggest(message.word).slice(0, 6) : [],
      })
      break
  }
})
