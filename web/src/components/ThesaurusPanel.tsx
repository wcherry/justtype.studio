import { useEffect, useState } from 'react'
import { ApiError, api } from '../api/client'
import type { PartOfSpeech, ThesaurusEntry } from '../api/types'

const POS_LABEL: Record<PartOfSpeech, string> = {
  noun: 'noun',
  verb: 'verb',
  adjective: 'adjective',
  adverb: 'adverb',
}

/** Gives a replacement the capitals of the word it replaces. */
export function matchCase(replacement: string, original: string) {
  if (original.length > 1 && original === original.toUpperCase()) return replacement.toUpperCase()
  if (original[0] && original[0] === original[0].toUpperCase() && original[0] !== original[0].toLowerCase()) {
    return replacement[0].toUpperCase() + replacement.slice(1)
  }
  return replacement
}

/**
 * The thesaurus: a word's meanings, most common first, and the words for
 * each. Clicking a word puts it in place of the one in the text; ⌥-clicking
 * looks it up instead, and Back retraces the steps.
 *
 * The lookups go to this app's server and no further.
 */
export function ThesaurusPanel({
  word, onUse,
}: {
  /** The word to start from: the one at the caret, or the one right-clicked. */
  word: string
  /** Put a word in place of the one in the text. Absent when there is nowhere to put it. */
  onUse?: (replacement: string) => string | null
}) {
  const [trail, setTrail] = useState<string[]>(word ? [word] : [])
  const [query, setQuery] = useState(word)
  const [entry, setEntry] = useState<ThesaurusEntry | null>(null)
  const [status, setStatus] = useState<'idle' | 'loading' | 'missing' | 'failed'>('idle')
  const [note, setNote] = useState<string | null>(null)
  const current = trail[trail.length - 1] ?? ''

  // A new word from outside starts a new trail.
  useEffect(() => {
    setTrail(word ? [word] : [])
    setQuery(word)
  }, [word])

  useEffect(() => {
    if (!current) {
      setEntry(null)
      setStatus('idle')
      return
    }
    let stale = false
    setStatus('loading')
    api.words.thesaurus(current).then(
      (found) => {
        if (stale) return
        setEntry(found)
        setStatus('idle')
      },
      (e) => {
        if (stale) return
        setEntry(null)
        setStatus(e instanceof ApiError && e.status === 404 ? 'missing' : 'failed')
      },
    )
    return () => {
      stale = true
    }
  }, [current])

  const lookUp = (next: string) => {
    const clean = next.trim()
    if (!clean || clean === current) return
    setTrail((t) => [...t, clean])
    setQuery(clean)
    setNote(null)
  }

  const choose = (event: React.MouseEvent, candidate: string) => {
    if (event.altKey || !onUse) return lookUp(candidate)
    setNote(onUse(candidate))
  }

  const chips = (label: string, words: string[]) =>
    words.length > 0 && (
      <div className="thesaurus__group">
        <span className="thesaurus__label">{label}</span>
        <div className="thesaurus__words">
          {words.map((w) => (
            <button
              key={w}
              type="button"
              className="thesaurus__word"
              title={onUse ? `Use “${w}” · ⌥-click to look it up` : `Look up “${w}”`}
              onClick={(e) => choose(e, w)}
            >
              {w}
            </button>
          ))}
        </div>
      </div>
    )

  return (
    <div className="thesaurus">
      <form
        className="thesaurus__search"
        onSubmit={(e) => {
          e.preventDefault()
          lookUp(query)
        }}
      >
        {trail.length > 1 && (
          <button
            type="button"
            className="linklike"
            onClick={() => {
              const back = trail.slice(0, -1)
              setTrail(back)
              setQuery(back[back.length - 1])
            }}
          >
            ← {trail[trail.length - 2]}
          </button>
        )}
        <input
          type="search"
          value={query}
          placeholder="Look up a word"
          aria-label="Look up a word"
          onChange={(e) => setQuery(e.target.value)}
        />
      </form>

      {onUse && trail.length > 0 && (
        <p className="field__hint">
          Click a word to use it in place of “{trail[0]}”. ⌥-click to look it up instead.
        </p>
      )}
      {note && <p className="error">{note}</p>}
      {status === 'loading' && <p className="field__hint">Looking…</p>}
      {status === 'missing' && <p className="field__hint">Nothing in the thesaurus for “{current}”.</p>}
      {status === 'failed' && <p className="error">The thesaurus could not be reached.</p>}

      {entry && status === 'idle' && (
        <>
          {entry.base.toLowerCase() !== entry.word.toLowerCase() && (
            <p className="field__hint">Words for “{entry.base}”, the word “{entry.word}” is a form of.</p>
          )}
          <ol className="thesaurus__senses">
            {entry.senses.map((sense, i) => (
              <li key={i} className="thesaurus__sense">
                <p className="thesaurus__gloss">
                  <span className="thesaurus__pos">{POS_LABEL[sense.partOfSpeech]}</span> {sense.gloss}
                </p>
                {chips('Same', sense.synonyms)}
                {chips('Near', sense.related)}
                {chips('Opposite', sense.antonyms)}
              </li>
            ))}
          </ol>
        </>
      )}
      <p className="thesaurus__credit">From WordNet, Princeton University.</p>
    </div>
  )
}
