import { useEffect, useState } from 'react'
import type { Editor } from '@tiptap/react'
import { ApiError, api } from '../api/client'
import type { AssistTask, Scene } from '../api/types'
import { useAuth } from '../state/auth'
import { applySuggestion, snapshot, wordDiff } from '../editor/assist'
import type { AssistRange } from '../editor/assist'

/** Rewrites a writer reaches for often enough to be a click. */
export const REWRITES: { label: string; instruction: string }[] = [
  { label: 'Tighter', instruction: 'Tighten it: cut what is not pulling its weight, keep everything that is.' },
  { label: 'More vivid', instruction: 'Make it more vivid and concrete: sharper sensory detail, stronger verbs.' },
  { label: 'Plainer', instruction: 'Make it plainer and clearer, without losing the voice.' },
  { label: 'More tension', instruction: 'Raise the tension.' },
  { label: 'Vary the rhythm', instruction: 'Vary the length and rhythm of the sentences.' },
  { label: 'Show, don’t tell', instruction: 'Show rather than tell: turn stated feelings into action, detail and dialogue.' },
]

/** Something asked for from outside the panel — the selection menu. */
export interface AssistAsk {
  task: AssistTask
  instruction?: string
  /** Changes with every request, so asking the same thing twice asks twice. */
  nonce: number
}

interface Result {
  task: AssistTask
  instruction: string
  text: string
  model: string
  range: AssistRange
  /** For a proofread: what the passage was, to show what changed. */
  passage: string
}

const VERB: Record<AssistTask, string> = {
  continue: 'Writing on…',
  rewrite: 'Rewriting…',
  proofread: 'Proofreading…',
  ask: 'Thinking…',
}

/**
 * The writing assistant: Claude, asked to continue from the cursor, rewrite or
 * proofread the selected passage, or answer a question about it.
 *
 * Off until the writer turns it on, and the panel says what that means first.
 * What comes back is shown here, and goes into the scene only when the writer
 * puts it there — as one step, so ⌘Z takes it out again.
 */
export function AssistantPanel({
  editor, scene, writable, ask,
}: {
  editor: Editor | null
  scene: Scene | null
  writable: boolean
  ask: AssistAsk | null
}) {
  const { user, updateUser } = useAuth()
  const [busy, setBusy] = useState<AssistTask | null>(null)
  const [result, setResult] = useState<Result | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [instruction, setInstruction] = useState('')
  const [question, setQuestion] = useState('')
  const [copied, setCopied] = useState(false)
  // Kept current so the buttons say what they would act on.
  const [hasSelection, setHasSelection] = useState(() => !!editor && !editor.state.selection.empty)
  useEffect(() => {
    if (!editor) return
    const update = () => setHasSelection(!editor.state.selection.empty)
    update()
    editor.on('selectionUpdate', update)
    return () => {
      editor.off('selectionUpdate', update)
    }
  }, [editor])

  const allowed = !!user?.assistantAllowedAt

  const run = async (task: AssistTask, said = '') => {
    if (!editor || !scene) return
    const taken = snapshot(editor)
    setBusy(task)
    setError(null)
    setResult(null)
    setCopied(false)
    try {
      const view = await api.assistant.assist(scene.id, {
        task,
        instruction: said,
        before: taken.before,
        selection: task === 'continue' ? '' : taken.selection,
        after: taken.after,
      })
      setResult({
        task,
        instruction: said,
        text: view.text,
        model: view.model,
        // Continuing writes at the caret, and leaves a selection alone.
        range: task === 'continue' ? { from: taken.range.to, to: taken.range.to, original: '' } : taken.range,
        passage: taken.selection,
      })
    } catch (e) {
      setError(
        e instanceof ApiError && e.status === 429 ? e.message
          : e instanceof Error ? e.message : 'The assistant could not answer that',
      )
    } finally {
      setBusy(null)
    }
  }

  // Asked from the selection menu.
  useEffect(() => {
    if (!ask || !allowed) return
    if (ask.task === 'rewrite') setInstruction(ask.instruction ?? '')
    if (ask.task === 'ask') setQuestion(ask.instruction ?? '')
    void run(ask.task, ask.instruction ?? '')
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ask?.nonce, allowed])

  if (!allowed) {
    return (
      <div className="assistant">
        <p>
          The writing assistant is Claude, a model Anthropic runs. It can continue from the
          cursor, rewrite or proofread a passage, or answer a question about it. Turning it on
          means that what you hand it — the passage you select, or the scene up to the cursor,
          with some of the scene around it — is sent to Anthropic when you ask, and only then.
        </p>
        <p>
          What it writes is a suggestion. Nothing goes into your manuscript unless you put it
          there, and you can take it out again with Undo.
        </p>
        <p className="field__hint">This is separate from summaries. You can turn it off again in Settings.</p>
        {error && <p className="error">{error}</p>}
        <button
          type="button"
          onClick={async () => {
            try {
              updateUser(await api.assistant.allow())
            } catch (e) {
              setError(e instanceof Error ? e.message : 'Could not turn it on')
            }
          }}
        >
          Turn on the writing assistant
        </button>
      </div>
    )
  }

  if (!scene) return <p className="empty">Open a scene to use the assistant.</p>
  if (!writable) return <p className="field__hint">The assistant works on scenes you can change.</p>

  const apply = () => {
    if (!editor || !result) return
    const problem = applySuggestion(editor, result.range, result.text, { continuing: result.task === 'continue' })
    if (problem) setError(problem)
    else setResult(null)
  }

  const diff = result?.task === 'proofread' ? wordDiff(result.passage, result.text) : null
  const unchanged = result?.task === 'proofread' && result.passage.trim() === result.text.trim()

  return (
    <div className="assistant">
      <section className="assistant__section">
        <h4>At the cursor</h4>
        <button type="button" disabled={busy !== null} onClick={() => void run('continue')}>
          {busy === 'continue' ? VERB.continue : 'Continue from here'}
        </button>
      </section>

      <section className="assistant__section">
        <h4>The selected passage</h4>
        {!hasSelection && <p className="field__hint">Select a passage in the scene to rewrite or proofread it.</p>}
        <div className="assistant__row">
          <button type="button" disabled={!hasSelection || busy !== null} onClick={() => void run('proofread')}>
            {busy === 'proofread' ? VERB.proofread : 'Proofread'}
          </button>
        </div>
        <div className="assistant__presets" role="group" aria-label="Rewrite it">
          {REWRITES.map((preset) => (
            <button
              key={preset.label}
              type="button"
              className="assistant__preset"
              disabled={!hasSelection || busy !== null}
              onClick={() => {
                setInstruction(preset.instruction)
                void run('rewrite', preset.instruction)
              }}
            >
              {preset.label}
            </button>
          ))}
        </div>
        <form
          className="assistant__form"
          onSubmit={(e) => {
            e.preventDefault()
            if (instruction.trim()) void run('rewrite', instruction.trim())
          }}
        >
          <textarea
            rows={2}
            value={instruction}
            maxLength={500}
            placeholder="Or say how: “in her voice, not his”, “less formal”, “cut to half”…"
            onChange={(e) => setInstruction(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) e.currentTarget.form?.requestSubmit()
            }}
          />
          <button type="submit" disabled={!hasSelection || !instruction.trim() || busy !== null}>
            {busy === 'rewrite' ? VERB.rewrite : 'Rewrite'}
          </button>
        </form>
      </section>

      <section className="assistant__section">
        <h4>Ask</h4>
        <form
          className="assistant__form"
          onSubmit={(e) => {
            e.preventDefault()
            if (question.trim()) void run('ask', question.trim())
          }}
        >
          <textarea
            rows={2}
            value={question}
            maxLength={500}
            placeholder={hasSelection ? 'About the selected passage: “does this drag?”' : 'About this scene: “a name for the innkeeper?”'}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) e.currentTarget.form?.requestSubmit()
            }}
          />
          <button type="submit" disabled={!question.trim() || busy !== null}>
            {busy === 'ask' ? VERB.ask : 'Ask'}
          </button>
        </form>
      </section>

      {error && <p className="error" role="alert">{error}</p>}

      {result && (
        <section className="assistant__result" aria-live="polite">
          {unchanged ? (
            <p className="field__hint">Nothing to correct.</p>
          ) : diff ? (
            <p className="assistant__text">
              {diff.map((part, i) => (
                part.kind === 'same' ? <span key={i}>{part.text}</span>
                  : part.kind === 'add' ? <ins key={i} className="diff__insert">{part.text}</ins>
                    : <del key={i} className="diff__delete">{part.text}</del>
              ))}
            </p>
          ) : (
            <p className="assistant__text">{result.text}</p>
          )}
          <p className="field__hint">Suggested by {result.model}. Nothing has changed in your scene.</p>
          <div className="assistant__row">
            {result.task !== 'ask' && !unchanged && (
              <button type="button" onClick={apply}>
                {result.task === 'continue' ? 'Insert at the cursor' : 'Replace the passage'}
              </button>
            )}
            <button
              type="button"
              className="linklike"
              onClick={() => {
                void navigator.clipboard?.writeText(result.text).then(() => setCopied(true))
              }}
            >
              {copied ? 'Copied' : 'Copy'}
            </button>
            {result.task !== 'proofread' && (
              <button
                type="button"
                className="linklike"
                disabled={busy !== null}
                onClick={() => {
                  // Asked again of the same passage, wherever the selection is now.
                  if (!editor) return
                  editor.chain().setTextSelection({ from: result.range.from, to: result.range.to }).run()
                  void run(result.task, result.instruction)
                }}
              >
                Try again
              </button>
            )}
            <button type="button" className="linklike" onClick={() => setResult(null)}>Discard</button>
          </div>
        </section>
      )}
    </div>
  )
}
