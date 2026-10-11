import { useState } from 'react'
import { staff } from '../../api/client'
import type { TestResult } from '../../api/types'
import { errorText, useLoad } from './common'

const ACTIONS: [string, string][] = [
  ['block', 'Block — refuse the post'],
  ['hold', 'Hold — keep it for review'],
  ['flag', 'Flag — publish it and queue it'],
  ['mask', 'Mask — publish it with •••'],
]

const SURFACE_LABEL: Record<string, string> = {
  comment: 'comments',
  question: 'reader questions',
  answer: 'reader answers',
  entry: 'Daily Prompts answers',
  critique: 'critiques',
  display_name: 'display names',
  bio: 'bios',
  about_author: 'about the author',
  publication_title: 'publication titles',
  blurb: 'blurbs and prompts',
}

/** The list of words that are never acceptable in public, what happens to each, and where. */
export function AdminWordsPage() {
  const { data, error, reload } = useLoad(() => staff.words(), [])
  const [term, setTerm] = useState('')
  const [match, setMatch] = useState('word')
  const [action, setAction] = useState('hold')
  const [applies, setApplies] = useState<string[]>(['comment', 'critique'])
  const [note, setNote] = useState('')
  const [allow, setAllow] = useState('')
  const [text, setText] = useState('')
  const [result, setResult] = useState<TestResult | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const [csv, setCsv] = useState('')

  const run = async (work: () => Promise<unknown>, said?: string) => {
    setProblem(null)
    try {
      await work()
      if (said) setMessage(said)
      await reload()
    } catch (e) {
      setProblem(errorText(e))
    }
  }
  const surfaces = data?.surfaces ?? Object.keys(SURFACE_LABEL)

  return (
    <main className="feed admin">
      <header className="feed__header"><h1>Word filter</h1></header>
      <p className="field__hint">
        Only public text is ever checked: comments, reader questions and answers, Daily Prompts answers and critiques, names, bios and
        publication blurbs. Manuscripts, drafts, notebooks and unsubmitted answers are never scanned. There are no regular
        expressions, and the word is never named to the person whose post is refused.
      </p>
      {(error || problem) && <p className="error">{problem ?? error}</p>}
      {message && <p className="banner" role="status">{message}</p>}

      <section className="prompts__section">
        <h2>Add a term</h2>
        <form
          className="admin__reason"
          onSubmit={(e) => {
            e.preventDefault()
            void run(async () => {
              await staff.addTerm({ term, matchKind: match, action, appliesTo: applies, note })
              setTerm('')
              setNote('')
            })
          }}
        >
          <label>Term<input value={term} onChange={(e) => setTerm(e.target.value)} required /></label>
          <div className="settings__row">
            <label>
              Match
              <select value={match} onChange={(e) => setMatch(e.target.value)}>
                <option value="word">Whole word</option>
                <option value="substring">Anywhere in a word</option>
              </select>
            </label>
            <label>
              Action
              <select value={action} onChange={(e) => setAction(e.target.value)}>
                {ACTIONS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
              </select>
            </label>
          </div>
          <fieldset className="admin__surfaces">
            <legend>Applies to</legend>
            {surfaces.map((s) => (
              <label key={s} className="auth__check">
                <input
                  type="checkbox"
                  checked={applies.includes(s)}
                  onChange={(e) => setApplies(e.target.checked ? [...applies, s] : applies.filter((x) => x !== s))}
                />
                <span>{SURFACE_LABEL[s] ?? s}</span>
              </label>
            ))}
          </fieldset>
          <label>Note (for staff)<input value={note} onChange={(e) => setNote(e.target.value)} /></label>
          <button type="submit" disabled={!term.trim() || applies.length === 0}>Add</button>
        </form>
      </section>

      <section className="prompts__section">
        <h2>The list ({data?.terms.length ?? 0})</h2>
        <div className="admin__table-wrap">
          <table className="admin__table">
            <thead><tr><th>Term</th><th>Match</th><th>Action</th><th>Applies to</th><th>Note</th><th /></tr></thead>
            <tbody>
              {data?.terms.map((t) => {
                const where = (() => { try { return JSON.parse(t.appliesTo) as string[] } catch { return [] } })()
                return (
                  <tr key={t.id}>
                    <td>{t.term}</td>
                    <td>{t.matchKind === 'word' ? 'whole word' : 'substring'}</td>
                    <td>
                      <select
                        aria-label={`Action for ${t.term}`}
                        value={t.action}
                        onChange={(e) => void run(() => staff.updateTerm(t.id, { action: e.target.value }))}
                      >
                        {ACTIONS.map(([v]) => <option key={v} value={v}>{v}</option>)}
                      </select>
                    </td>
                    <td>{where.map((s) => SURFACE_LABEL[s] ?? s).join(', ')}</td>
                    <td>{t.note}</td>
                    <td><button type="button" className="linklike" onClick={() => void run(() => staff.deleteTerm(t.id))}>Remove</button></td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
        {data?.terms.length === 0 && <p className="empty">The list is empty, which is where it starts.</p>}
      </section>

      <section className="prompts__section">
        <h2>Allowed words</h2>
        <p className="field__hint">Words that contain a term but are fine (the Scunthorpe problem): "cucumber", "document".</p>
        <ul className="admin__chips">
          {data?.allow.map((w) => (
            <li key={w}>{w} <button type="button" className="linklike" onClick={() => void run(() => staff.unallowWord(w))}>×</button></li>
          ))}
        </ul>
        <form className="admin__add" onSubmit={(e) => { e.preventDefault(); void run(async () => { await staff.allowWord(allow); setAllow('') }) }}>
          <input aria-label="Allowed word" value={allow} onChange={(e) => setAllow(e.target.value)} placeholder="cucumber" />
          <button type="submit" disabled={!allow.trim()}>Allow</button>
        </form>
      </section>

      <section className="prompts__section">
        <h2>Test box</h2>
        <textarea aria-label="Text to test" rows={3} value={text} onChange={(e) => setText(e.target.value)} placeholder="Paste some text…" />
        <button
          type="button"
          onClick={() => void staff.testWords(text).then(setResult).catch((e) => setProblem(errorText(e)))}
          disabled={!text.trim()}
        >
          Test it
        </button>
        {result && (
          <div className="admin__test">
            <p>
              {result.action ? <>This would <strong>{result.action}</strong>.</> : 'Nothing matches.'}
            </p>
            {result.matches.map((m, i) => <p key={i} className="field__hint">“{m.term}” → {m.action}{m.note && ` (${m.note})`}</p>)}
            {result.masked && <p>Published as: {result.masked}</p>}
          </div>
        )}
      </section>

      <section className="prompts__section">
        <h2>Import, export and re-scan</h2>
        <div className="admin__actions">
          <button
            type="button"
            onClick={async () => {
              const file = await staff.exportWords()
              const url = URL.createObjectURL(new Blob([file], { type: 'text/csv' }))
              const a = document.createElement('a')
              a.href = url
              a.download = 'word-filter.csv'
              a.click()
              URL.revokeObjectURL(url)
            }}
          >
            Export as CSV
          </button>
          <button
            type="button"
            onClick={() => void staff.rescan().then((r) => setMessage(`Re-scanned the last 30 days: ${r.checked} checked, ${r.flagged} flagged for review.`)).catch((e) => setProblem(errorText(e)))}
          >
            Re-scan the last 30 days
          </button>
        </div>
        <label>
          Import CSV (term,match,action,applies_to,note — applies_to separated by ;)
          <textarea rows={4} value={csv} onChange={(e) => setCsv(e.target.value)} />
        </label>
        <button type="button" disabled={!csv.trim()} onClick={() => void run(async () => { const r = await staff.importWords(csv); setMessage(`Added ${r.added}.`); setCsv('') })}>Import</button>
        <p className="field__hint">Changing the list does not re-scan old posts by itself.</p>
      </section>
    </main>
  )
}
