import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import Placeholder from '@tiptap/extension-placeholder'
import { ApiError, prompts } from '../api/client'
import type { DocNode, PromptEntryView, PromptView } from '../api/types'
import { GoalBar } from '../components/GoalBar'
import { SelectionMenu, useSelectionMenu } from '../components/SelectionMenu'
import type { MenuTarget, SelectionAction } from '../components/SelectionMenu'
import { SidePanel } from '../components/SidePanel'
import { ToolButton, Toolbar } from '../components/Toolbar'
import { WritingPreferencesPanel } from '../components/WritingPreferences'
import { WritingRoomBar } from '../components/WritingRoomBar'
import { useDistractionFree } from '../editor/distractionFree'
import { FocusMode } from '../editor/focusMode'
import { useWritingPreferences } from '../editor/preferences'
import { AnswerTypography } from '../editor/typography'
import { TypewriterScrolling } from '../editor/typewriter'
import { useEntryAutosave } from '../editor/useEntryAutosave'
import { Spellcheck } from '../editor/spelling/spellcheck'
import { useSpelling } from '../editor/spelling/useSpelling'
import { useWordMenu } from '../editor/spelling/useWordMenu'
import { useSessionGoals } from '../editor/useSessionGoals'
import { entryKey, pendingFor, startQueueReplay } from '../offline/queue'
import { dayName, localTime } from '../prompts/days'
import { ToWorkDialog } from '../prompts/ToWorkDialog'
import { useStanding } from '../state/standing'

const SAVE_LABEL: Record<string, string> = {
  idle: '',
  dirty: 'Unsaved changes',
  saving: 'Saving…',
  saved: 'Saved',
  queued: 'Saved on this device',
  conflict: 'Needs your decision',
  error: 'Save failed — retrying on next edit',
}

const STATE_NOTE: Record<string, string> = {
  draft: 'A private draft: only you can see it.',
  submitted: 'Submitted: everyone in Daily Prompts can read it. Changes you make show as edited.',
  withdrawn: 'Withdrawn: private again. Its critiques are kept for when you submit it again.',
}

const words = (doc: DocNode | null | undefined): number => {
  const out: string[] = []
  const walk = (n: DocNode) => {
    if (n.text) out.push(n.text)
    n.content?.forEach((c) => {
      walk(c)
      out.push(' ')
    })
  }
  if (doc) walk(doc)
  return out.join('').split(/\s+/).filter(Boolean).length
}

/**
 * Answering a prompt, in the editor: one page, no outline, no chapters, no
 * versions. The goals bar, focus mode, typewriter scrolling, the writing room
 * (⌘⇧F) and the type settings work as they do on a book. No word limit.
 */
export function PromptWritePage() {
  const { day = '' } = useParams()
  const navigate = useNavigate()
  const { blocked } = useStanding()
  const [prompt, setPrompt] = useState<PromptView | null>(null)
  const [entry, setEntry] = useState<PromptEntryView | null>(null)
  const [enrolled, setEnrolled] = useState<boolean | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [ready, setReady] = useState(false)
  const [busy, setBusy] = useState(false)
  const [prefsOpen, setPrefsOpen] = useState(false)
  const [converting, setConverting] = useState(false)
  const [liveWords, setLiveWords] = useState(0)
  const [online, setOnline] = useState(() => navigator.onLine)

  const { preferences, update: setPreference, reset: resetPreferences, style: proseStyle } = useWritingPreferences()
  const editor = useEditor({
    extensions: [
      StarterKit,
      Underline,
      Placeholder.configure({ placeholder: 'Start anywhere. There is no word limit.' }),
      FocusMode,
      TypewriterScrolling,
      ...AnswerTypography,
      Spellcheck,
    ],
    content: '',
    // The app checks spelling itself, as on a book; the browser's own checker
    // would mark the same words twice, differently, with its own menu.
    editorProps: { attributes: { spellcheck: 'false' } },
  })
  useEffect(() => {
    if (!editor) return
    const focus = editor.extensionManager.extensions.find((e) => e.name === 'focusMode')
    const typewriter = editor.extensionManager.extensions.find((e) => e.name === 'typewriterScrolling')
    if (focus) focus.options.enabled = preferences.focusMode
    if (typewriter) typewriter.options.enabled = preferences.typewriter
    editor.view.dispatch(editor.state.tr)
  }, [editor, preferences.focusMode, preferences.typewriter])

  // The writer's own dictionary, as on a book. An answer has no notebook, so
  // no character names.
  const spelling = useSpelling(editor, {
    enabled: preferences.spellcheck,
    language: preferences.spellingLanguage,
    workId: '',
  })
  const contextMenu = useSelectionMenu(editor, true, true)
  const wordActions = useWordMenu(editor, contextMenu.target, {
    spelling,
    spellcheck: preferences.spellcheck,
    writable: !!enrolled,
    onError: setError,
  })

  /** Right-click: spelling and synonyms for a word, and Copy for a selection. */
  const menuActions = (target: MenuTarget): SelectionAction[] => {
    const actions = wordActions()
    const quote = target.selection?.quote
    if (quote) {
      actions.push({
        key: 'copy', label: 'Copy', hint: '⌘C', divider: actions.length > 0,
        onSelect: () => void navigator.clipboard?.writeText(quote),
      })
    }
    return actions
  }

  const page = useDistractionFree()
  const roomStyle = {
    ...proseStyle,
    '--prose-size': `${(preferences.fontSize * preferences.roomZoom) / 100}px`,
    '--measure': `${preferences.roomWidth}%`,
  } as React.CSSProperties

  const onSaved = useCallback((saved: PromptEntryView) => setEntry(saved), [])
  const save = useEntryAutosave({ day, entry, editor, onSaved })
  const { summary, sessionWords, refresh: refreshGoals, setDailyGoal, setWeeklyGoal } = useSessionGoals(
    ready ? liveWords : null,
    entryKey(day),
  )
  useEffect(() => {
    if (save.state === 'saved') void refreshGoals()
  }, [save.state, refreshGoals])

  useEffect(() => {
    const stop = startQueueReplay()
    const sync = () => setOnline(navigator.onLine)
    window.addEventListener('online', sync)
    window.addEventListener('offline', sync)
    return () => {
      stop()
      window.removeEventListener('online', sync)
      window.removeEventListener('offline', sync)
    }
  }, [])

  // The prompt, the answer so far, and anything left on this device from
  // writing offline, which is newer than what the server has.
  useEffect(() => {
    if (!editor) return
    let live = true
    setReady(false)
    ;(async () => {
      try {
        const [p, e, mine] = await Promise.all([prompts.day(day), prompts.enrollment(), prompts.entry(day)])
        if (!live) return
        setPrompt(p)
        setEnrolled(e.enrolled)
        setEntry(mine)
        const queued = await pendingFor(entryKey(day))
        const content = queued?.content ?? mine?.content ?? null
        editor.commands.setContent((content ?? '') as never, false)
        setLiveWords(words(content as DocNode | null))
        editor.setEditable(e.enrolled)
        setReady(true)
      } catch (err) {
        if (!live) return
        setError(err instanceof ApiError && err.status === 404 ? 'There is no prompt for that day.' : err instanceof Error ? err.message : 'Could not open it')
      }
    })()
    return () => {
      live = false
    }
  }, [editor, day])

  useEffect(() => {
    if (!editor) return
    const onUpdate = () => {
      setLiveWords(words(editor.getJSON() as DocNode))
      save.schedule()
    }
    editor.on('update', onUpdate)
    return () => {
      editor.off('update', onUpdate)
    }
  }, [editor, save.schedule]) // eslint-disable-line react-hooks/exhaustive-deps

  const act = async (work: () => Promise<PromptEntryView | void>) => {
    setBusy(true)
    setError(null)
    try {
      await save.flush()
      const next = await work()
      if (next) setEntry(next)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not do that')
    } finally {
      setBusy(false)
    }
  }

  const state = entry?.state ?? 'none'
  const title = useMemo(() => (prompt ? dayName(prompt.day) : 'Daily prompt'), [prompt])

  return (
    <main className="editor editor--single">
      {page.on && (
        <WritingRoomBar
          title={prompt?.body ?? title}
          wordCount={liveWords}
          saveLabel={SAVE_LABEL[save.state]}
          zoom={preferences.roomZoom}
          onZoom={(percent) => setPreference('roomZoom', percent)}
          width={preferences.roomWidth}
          onWidth={(percent) => setPreference('roomWidth', percent)}
          onLeave={page.leave}
        />
      )}
      <section className="editor__main">
        <header className="editor__header prompt-write__header">
          <div>
            <p className="editor__meta">
              <Link to="/prompts">Daily Prompts</Link> · {title}
              {prompt && Date.parse(prompt.closesAt) > Date.now() && ` · open until ${localTime(prompt.closesAt)}`}
            </p>
            {prompt && <blockquote className="prompt-write__prompt">{prompt.body}</blockquote>}
            {entry && <p className="field__hint">{STATE_NOTE[state]}{entry.late && ' Submitted late.'}</p>}
          </div>
          <div className="editor__header-actions">
            <span className="field__hint" aria-live="polite">{SAVE_LABEL[save.state]}</span>
            {enrolled && (state === 'none' || state === 'draft' || state === 'withdrawn') && (
              <button
                type="button"
                disabled={busy || liveWords === 0 || !online || !!blocked('discuss')}
                title={!online ? 'Back online, then submit' : blocked('discuss') ?? undefined}
                onClick={() => void act(() => prompts.submit(day))}
              >
                {state === 'withdrawn' ? 'Submit again' : 'Submit'}
              </button>
            )}
            {enrolled && state === 'submitted' && (
              <>
                <Link to={`/prompts/${day}/entries/${entry!.id}`}>Read it</Link>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    if (window.confirm('Withdraw it? It goes back to a private draft; its critiques are kept.')) {
                      void act(() => prompts.withdraw(day))
                    }
                  }}
                >
                  Withdraw
                </button>
              </>
            )}
            {entry && (
              <button type="button" disabled={busy} onClick={() => void save.flush().then(() => setConverting(true))}>
                Turn into a project
              </button>
            )}
            {entry && (
              <button
                type="button"
                className="danger"
                disabled={busy}
                onClick={() => {
                  if (window.confirm('Delete this answer and every critique on it? This cannot be undone.')) {
                    void act(async () => {
                      await prompts.deleteEntry(day)
                      navigate('/prompts')
                    })
                  }
                }}
              >
                Delete
              </button>
            )}
          </div>
        </header>
        {entry?.convertedWork && (
          <p className="access-note">
            Turned into <Link to={`/works/${entry.convertedWork.id}`}>{entry.convertedWork.title}</Link>. Changes here do not reach it.
          </p>
        )}
        {enrolled === false && (
          <p className="access-note">
            <Link to="/prompts">Join Daily Prompts</Link> to write an answer.
            {entry && <> You can still <Link to={`/prompts/${day}/entries/${entry.id}`}>read yours</Link>.</>}
          </p>
        )}
        {!online && save.pending > 0 && (
          <p className="access-note" role="status">Offline: your words are saved on this device and go up when you are back.</p>
        )}
        {save.conflict && (
          <p className="access-note" role="alert">
            This answer changed somewhere else since you opened it.{' '}
            <button type="button" className="linklike" onClick={() => void save.keepMine()}>Keep this version</button>
            {' · '}
            <button type="button" className="linklike" onClick={() => window.location.reload()}>Load the other one</button>
          </p>
        )}
        {error && <p className="error">{error}</p>}

        <Toolbar editor={editor} canFormat={!!enrolled}>
          <ToolButton icon="room" label="Writing room: nothing but the words (⌘⇧F)" onClick={page.enter} />
          <ToolButton icon="info" label="Type and focus settings" pressed={prefsOpen} onClick={() => setPrefsOpen((o) => !o)} />
        </Toolbar>

        <GoalBar
          summary={summary}
          sessionWords={sessionWords}
          sceneWords={liveWords}
          sceneTarget={null}
          onSetDailyGoal={setDailyGoal}
          onSetWeeklyGoal={setWeeklyGoal}
          onSetSceneTarget={() => undefined}
          bookWords={liveWords}
          bookTarget={null}
          bookDeadline={null}
          canSetBookTarget={false}
          onSetBookTarget={() => undefined}
          pieceLabel="in this answer"
        />

        <div className="prose-scroll">
          <EditorContent editor={editor} className="prose prose--editable" style={page.on ? roomStyle : proseStyle} />
        </div>
      </section>

      {contextMenu.target && (
        <SelectionMenu target={contextMenu.target} actions={menuActions(contextMenu.target)} onClose={contextMenu.close} />
      )}
      {prefsOpen && (
        <SidePanel title="Writing" onClose={() => setPrefsOpen(false)}>
          <WritingPreferencesPanel preferences={preferences} onChange={setPreference} onReset={resetPreferences} />
        </SidePanel>
      )}
      {converting && entry && <ToWorkDialog entryId={entry.id} onClose={() => setConverting(false)} />}
    </main>
  )
}
