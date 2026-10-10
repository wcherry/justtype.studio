import { useState } from 'react'

/**
 * The words the writer has told the spellchecker are right. Names in the
 * book's notebook count as right without being here.
 */
export function DictionaryPanel({
  words, onAdd, onRemove, error,
}: {
  words: string[]
  onAdd: (word: string) => Promise<void>
  onRemove: (word: string) => Promise<void>
  error: string | null
}) {
  const [draft, setDraft] = useState('')
  return (
    <div className="dictionary">
      <p className="field__hint">
        Words you have told the spellchecker are right. They follow you to every book and
        device. Names in this book’s notebook are known without being added.
      </p>
      <form
        className="dictionary__add"
        onSubmit={(e) => {
          e.preventDefault()
          const word = draft.trim()
          if (!word) return
          void onAdd(word).then(() => setDraft(''))
        }}
      >
        <input
          value={draft}
          placeholder="Add a word"
          aria-label="Add a word"
          onChange={(e) => setDraft(e.target.value)}
        />
        <button type="submit" disabled={!draft.trim()}>Add</button>
      </form>
      {error && <p className="error">{error}</p>}
      {words.length === 0 ? (
        <p className="empty">No words yet. Right-click an underlined word and choose “Add to dictionary”.</p>
      ) : (
        <ul className="dictionary__list">
          {words.map((word) => (
            <li key={word}>
              <span>{word}</span>
              <button
                type="button"
                className="linklike"
                aria-label={`Take “${word}” out of the dictionary`}
                onClick={() => void onRemove(word)}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
