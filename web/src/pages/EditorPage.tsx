import { useCallback, useEffect, useState } from 'react'
import { useParams } from 'react-router-dom'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import Placeholder from '@tiptap/extension-placeholder'

import { api } from '../api/client'
import type { Outline as OutlineType, Scene, SceneWithContent, VersionSummary } from '../api/types'
import { ActivityPanel } from '../components/ActivityPanel'
import { ChangesPanel } from '../components/ChangesPanel'
import { CollaboratorsPanel } from '../components/CollaboratorsPanel'
import { FindPanel } from '../components/FindPanel'
import { SummaryPanel } from '../components/SummaryPanel'
import { ConflictBanner } from '../components/ConflictBanner'
import { Corkboard } from '../components/Corkboard'
import { FrontMatter } from '../components/FrontMatter'
import { GoalBar } from '../components/GoalBar'
import { Outline } from '../components/Outline'
import { SceneDetails } from '../components/SceneDetails'
import { StoryBible } from '../components/StoryBible'
import { TaskList } from '../components/TaskList'
import { RevisionPanel } from '../components/RevisionPanel'
import { Menu } from '../components/Menu'
import { SidePanel } from '../components/SidePanel'
import { Toolbar } from '../components/Toolbar'
import { WritingPreferencesPanel } from '../components/WritingPreferences'
import { VersionPanel } from '../components/VersionPanel'
import { FocusMode } from '../editor/focusMode'
import { useDistractionFree } from '../editor/distractionFree'
import { useWritingPreferences } from '../editor/preferences'
import { ProseTypography } from '../editor/typography'
import { TypewriterScrolling } from '../editor/typewriter'
import { useAutosave } from '../editor/useAutosave'
import { useSessionGoals } from '../editor/useSessionGoals'
import { cache } from '../offline/cache'
import { pendingFor, startQueueReplay } from '../offline/queue'
import { permissions } from '../state/permissions'
import { useAuth } from '../state/auth'
import { here, useLive, usePresence } from '../live'

const SAVE_LABEL: Record<string, string> = {
  idle: '',
  dirty: 'Unsaved changes',
  saving: 'Saving…',
  saved: 'Saved',
  // Not an error: the text is on this device and goes up on reconnect.
  queued: 'Saved on this device',
  conflict: 'Needs your decision',
  error: 'Save failed — retrying on next edit',
}

/** The side panels; one is open at a time, or none. */
type Panel =
  | 'none'
  | 'versions'
  | 'history'
  | 'prefs'
  | 'front'
  | 'details'
  | 'bible'
  | 'changes'
  | 'tasks'
  | 'people'
  | 'activity'
  | 'find'
  | 'summary'

// Which header menu each panel is reached from, so the menu shows it is open.
const SCENE_PANELS: Panel[] = ['details', 'changes', 'history', 'summary']
const BOOK_PANELS: Panel[] = ['find', 'bible', 'tasks', 'front', 'versions']
const PEOPLE_PANELS: Panel[] = ['people', 'activity']

export function EditorPage() {
  const { workId = '' } = useParams()
  const [outline, setOutline] = useState<OutlineType | null>(null)
  const [versions, setVersions] = useState<VersionSummary[]>([])
  const [scene, setScene] = useState<Scene | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [panel, setPanel] = useState<Panel>('none')
  const toggle = (which: Panel) => setPanel((p) => (p === which ? 'none' : which))
  const closePanel = () => setPanel('none')
  const [view, setView] = useState<'write' | 'corkboard'>('write')
  /** What the writer has selected, in the same positions a proposal records. */
  const [selection, setSelection] = useState<{ from: number; to: number; quote: string } | null>(null)
  const [online, setOnline] = useState(() => navigator.onLine)
  /** Bumped after every accepted save, so the history list stays current. */
  const [savedAt, setSavedAt] = useState(0)

  const { preferences, update: setPreference, reset: resetPreferences, style: proseStyle } =
    useWritingPreferences()

  const editor = useEditor({
    extensions: [
      StarterKit,
      Underline,
      Placeholder.configure({ placeholder: 'Begin where it is already happening…' }),
      FocusMode,
      TypewriterScrolling,
      ProseTypography,
    ],
    content: '',
  })

  // Both read their options off the extension rather than from React state, so
  // toggling one has to nudge the view into recomputing.
  useEffect(() => {
    if (!editor) return
    const focus = editor.extensionManager.extensions.find((e) => e.name === 'focusMode')
    const typewriter = editor.extensionManager.extensions.find((e) => e.name === 'typewriterScrolling')
    if (focus) focus.options.enabled = preferences.focusMode
    if (typewriter) typewriter.options.enabled = preferences.typewriter
    editor.view.dispatch(editor.state.tr)
  }, [editor, preferences.focusMode, preferences.typewriter])

  const page = useDistractionFree()
  // Summaries exist only where the server has been set up to write them.
  const [summariesOn, setSummariesOn] = useState(false)
  useEffect(() => {
    api.features().then((f) => setSummariesOn(f.summaries)).catch(() => setSummariesOn(false))
  }, [])
  const can = permissions(outline?.access)
  const { user } = useAuth()

  // Who else is writing the book right now, and where.
  const room = `work:${workId}`
  const present = usePresence(room).filter((p) => p.id !== user?.id)
  useEffect(() => {
    here(room, scene?.id ?? null)
  }, [room, scene?.id])
  /** Someone else saved the scene open here: their text is newer than ours. */
  const [savedElsewhere, setSavedElsewhere] = useState<string | null>(null)
  useLive(room, (notice) => {
    const by = notice.by as { id: string; displayName: string } | undefined
    if (notice.type === 'scene_saved' && notice.sceneId === scene?.id && by && by.id !== user?.id) {
      setSavedElsewhere(by.displayName)
    }
  })
  useEffect(() => setSavedElsewhere(null), [scene?.id])
  // Someone handed only some chapters, or here to read or suggest, gets the
  // text without a caret: still selectable, so an editor can point at words.
  const writable = scene ? can.canWrite(scene.chapterId) : false
  useEffect(() => {
    if (editor && editor.isEditable !== writable) editor.setEditable(writable)
  }, [editor, writable])

  const onSaved = useCallback((workWordCount: number) => {
    setSavedAt(Date.now())
    setOutline((current) => (current ? { ...current, wordCount: workWordCount } : current))
  }, [])

  const {
    state: saveState, wordCount, setWordCount, pending, conflict, schedule, flush, resolveWith,
  } = useAutosave({ scene, editor, onSaved })

  const { summary, sessionWords, refresh: refreshGoals, setDailyGoal, setWeeklyGoal } = useSessionGoals(
    outline?.wordCount ?? null,
    workId,
  )

  // Today's total moves with every accepted save, so the bar has to follow.
  useEffect(() => {
    if (saveState === 'saved') void refreshGoals()
  }, [saveState, refreshGoals])

  // Replay anything queued from a previous session as soon as the editor opens,
  // and again whenever the network returns.
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

  const loadOutline = useCallback(async () => {
    try {
      const next = await api.works.outline(workId)
      setOutline(next)
      cache.putOutline(workId, next)
      setError(null)
      return next
    } catch (e) {
      // Offline: the cached structure is enough to get the writer back to the
      // scene they were in, where their queued text is waiting.
      const fallback = cache.outline(workId)
      if (fallback) {
        setOutline(fallback)
        return fallback
      }
      setError(e instanceof Error ? e.message : 'Could not open that work')
      return null
    }
  }, [workId])

  const loadVersions = useCallback(async () => {
    setVersions(await api.versions.list(workId).catch(() => []))
  }, [workId])

  // Open the work, then drop the writer straight into the first scene.
  useEffect(() => {
    void (async () => {
      const next = await loadOutline()
      await loadVersions()
      const first = next?.chapters.flatMap((c) => c.scenes)[0]
      if (first) void openScene(first)
    })()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workId])

  const openScene = useCallback(
    async (target: Scene) => {
      if (saveState === 'dirty') await flush(true)
      let full: SceneWithContent
      try {
        full = await api.scenes.get(target.id)
        cache.putScene(full)
      } catch (e) {
        const fallback = cache.scene(target.id)
        if (!fallback) {
          setError(
            'That scene has not been opened on this device yet, so it is not available offline.',
          )
          return
        }
        full = fallback
      }
      // Unsynced work outranks both the server copy and the cache. Without this,
      // reopening a scene edited offline would show the older text, and the next
      // keystroke would be based on it — quietly undoing the recovered writing
      // the moment the queue flushed.
      const queued = await pendingFor(target.id)
      const content = queued?.content ?? full.content ?? { type: 'doc', content: [] }
      setScene(full)
      setWordCount(full.wordCount)
      editor?.commands.setContent(content as never, false)
    },
    [editor, flush, saveState, setWordCount],
  )

  // The editor instance outlives scene changes, so wire the change handler once
  // it exists and re-wire whenever the active scene changes.
  useEffect(() => {
    if (!editor) return
    const handler = () => schedule()
    editor.on('update', handler)
    return () => {
      editor.off('update', handler)
    }
  }, [editor, schedule])

  // Selection is tracked for suggestions: a proposal records the same
  // ProseMirror positions the editor reports here.
  useEffect(() => {
    if (!editor) return
    const handler = () => {
      const { from, to, empty } = editor.state.selection
      setSelection(
        empty ? null : { from, to, quote: editor.state.doc.textBetween(from, to, ' ') },
      )
    }
    editor.on('selectionUpdate', handler)
    return () => {
      editor.off('selectionUpdate', handler)
    }
  }, [editor])

  /** The scene after this one in the same chapter, if there is one. */
  const nextScene = (() => {
    if (!scene || !outline) return null
    const chapter = outline.chapters.find((c) => c.id === scene.chapterId)
    const index = chapter?.scenes.findIndex((s) => s.id === scene.id) ?? -1
    return index >= 0 ? chapter?.scenes[index + 1] ?? null : null
  })()

  const splitHere = async () => {
    if (!scene || !editor) return
    // Anything unsaved has to land first: the server checks the hash, and it
    // would refuse a split decided against text it has never seen.
    await flush(true)
    const at = editor.state.selection.$from.depth > 0
      ? editor.state.doc.resolve(editor.state.selection.from).index(0)
      : 0
    try {
      const fresh = await api.scenes.get(scene.id)
      const result = await api.scenes.split(scene.id, at, fresh.contentHash)
      await loadOutline()
      void openScene(result.first)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not split that scene')
    }
  }

  const mergeNext = async () => {
    if (!scene || !nextScene) return
    await flush(true)
    try {
      const [mine, theirs] = await Promise.all([
        api.scenes.get(scene.id),
        api.scenes.get(nextScene.id),
      ])
      const merged = await api.scenes.merge(
        scene.id, nextScene.id, mine.contentHash, theirs.contentHash,
      )
      await loadOutline()
      void openScene(merged)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not merge those scenes')
    }
  }

  if (error) return <main className="editor"><p className="error">{error}</p></main>
  if (!outline) return <main className="editor"><p className="empty">Opening…</p></main>

  return (
    <main className="editor">
      {page.on && (
        <>
          {/* The pointer at the top edge brings this back; otherwise it is
              not there at all. */}
          <div className="page-bar" role="toolbar" aria-label="Page">
            <span className="page-bar__title">{scene?.title ?? outline.title}</span>
            <span className="page-bar__meta">{wordCount.toLocaleString()} words</span>
            <span className={`save save--${saveState}`}>{SAVE_LABEL[saveState]}</span>
            <span className="page-bar__spacer" />
            <button type="button" className="linklike" onClick={page.toggleOutline}>
              {page.outline ? 'Hide outline' : 'Outline'} <kbd>⌘⇧O</kbd>
            </button>
            <button type="button" className="linklike" onClick={page.leave}>
              Back to the desk <kbd>Esc</kbd>
            </button>
          </div>
          {/* Quiet enough to ignore, there for a glance. */}
          <div className={`page-status save--${saveState}`} aria-live="polite">
            {wordCount.toLocaleString()} · {SAVE_LABEL[saveState] || 'Saved'}
          </div>
        </>
      )}
      <Outline
        outline={outline}
        activeSceneId={scene?.id ?? null}
        onSelectScene={openScene}
        onAddChapter={async (title) => {
          await api.chapters.create(workId, title)
          void loadOutline()
        }}
        onAddScene={async (chapterId, title) => {
          const created = await api.scenes.create(chapterId, title)
          await loadOutline()
          void openScene(created)
        }}
        onMoveScene={async (chapterId, ids) => {
          await api.scenes.reorder(chapterId, ids)
          void loadOutline()
        }}
        onRenameScene={async (sceneId, title) => {
          await api.scenes.update(sceneId, { title })
          void loadOutline()
        }}
        canWrite={can.canWrite}
        canShape={can.canShape}
      />

      <section className="editor__main">
        <header className="editor__header">
          <div>
            <h1>{scene?.title ?? outline.title}</h1>
            <p className="editor__meta">
              {wordCount.toLocaleString()} words in this scene · {outline.wordCount.toLocaleString()} in the work
            </p>
          </div>
          <div className="editor__header-actions">
            <span className={`save save--${saveState}`}>
              {SAVE_LABEL[saveState]}
              {pending > 0 && saveState !== 'conflict' && (
                <span className="save__pending" title="waiting to reach the server">
                  {' '}· {pending} queued
                </span>
              )}
            </span>
            {scene && writable && (
              <select
                value={scene.status}
                onChange={async (e) => {
                  const status = e.target.value as Scene['status']
                  await api.scenes.update(scene.id, { status })
                  setScene({ ...scene, status })
                  void loadOutline()
                }}
              >
                <option value="outline">Outline</option>
                <option value="drafting">Drafting</option>
                <option value="revised">Revised</option>
                <option value="done">Done</option>
              </select>
            )}
            <div className="editor__tools" role="toolbar" aria-label="Writing">
              <div className="segmented" role="group" aria-label="View">
                <button
                  type="button"
                  className={view === 'write' ? 'is-active' : undefined}
                  aria-pressed={view === 'write'}
                  onClick={() => setView('write')}
                >
                  Write
                </button>
                <button
                  type="button"
                  className={view === 'corkboard' ? 'is-active' : undefined}
                  aria-pressed={view === 'corkboard'}
                  onClick={() => setView('corkboard')}
                >
                  Corkboard
                </button>
              </div>
              <button
                type="button"
                className="tool"
                title="Put everything away but the page (⌘⇧F)"
                onClick={page.enter}
              >
                Page
              </button>
              <button
                type="button"
                className={preferences.focusMode ? 'tool is-active' : 'tool'}
                aria-pressed={preferences.focusMode}
                title="Dim everything but the paragraph you are in"
                onClick={() => setPreference('focusMode', !preferences.focusMode)}
              >
                Focus
              </button>
              <button
                type="button"
                className={panel === 'prefs' ? 'tool is-active' : 'tool'}
                aria-pressed={panel === 'prefs'}
                title="Type, size and spacing"
                onClick={() => toggle('prefs')}
              >
                Aa
              </button>
            </div>
            <div className="editor__menus">
              <Menu
                label="Scene"
                active={SCENE_PANELS.includes(panel)}
                items={[
                  { key: 'details', label: 'Details', checked: panel === 'details', disabled: !scene, onSelect: () => toggle('details') },
                  { key: 'changes', label: 'Proposed changes', checked: panel === 'changes', disabled: !scene, onSelect: () => toggle('changes') },
                  { key: 'history', label: 'History', checked: panel === 'history', disabled: !scene, onSelect: () => toggle('history') },
                  ...(summariesOn && can.canWrite(scene?.chapterId ?? '')
                    ? [{ key: 'summary', label: 'Summarise…', checked: panel === 'summary', onSelect: () => toggle('summary') }]
                    : []),
                ]}
              />
              <Menu
                label="Book"
                active={BOOK_PANELS.includes(panel)}
                items={[
                  { key: 'find', label: 'Find', checked: panel === 'find', onSelect: () => toggle('find') },
                  { key: 'bible', label: 'Story bible', checked: panel === 'bible', onSelect: () => toggle('bible') },
                  { key: 'tasks', label: 'To do', checked: panel === 'tasks', onSelect: () => toggle('tasks') },
                  ...(can.isOwner
                    ? [
                        { key: 'front', label: 'Front matter', checked: panel === 'front', onSelect: () => toggle('front') },
                        { key: 'versions', label: 'Versions', checked: panel === 'versions', onSelect: () => toggle('versions') },
                      ]
                    : []),
                ]}
              />
              <Menu
                label="People"
                active={PEOPLE_PANELS.includes(panel)}
                items={[
                  { key: 'people', label: 'Who is on this book', checked: panel === 'people', onSelect: () => toggle('people') },
                  { key: 'activity', label: 'Activity', checked: panel === 'activity', onSelect: () => toggle('activity') },
                ]}
              />
            </div>
          </div>
        </header>

        {(!online || pending > 0) && saveState !== 'conflict' && (
          <p className="offline-note">
            {online
              ? `Can’t reach the server — ${pending} scene${pending === 1 ? '' : 's'} saved on this device, and going up as soon as it answers.`
              : 'Offline — your writing is being saved on this device and will go up when the connection returns.'}
          </p>
        )}

        {conflict && (
          <ConflictBanner
            conflict={conflict}
            onKeepMine={resolveWith}
            onTakeTheirs={(content, contentHash, words) => {
              editor?.commands.setContent(content as never, false)
              setScene((s) => (s ? { ...s, contentHash, wordCount: words } : s))
              setWordCount(words)
            }}
          />
        )}

        {present.length > 0 && (
          <p className="presence">
            {present.map((person) => {
              const where = outline.chapters
                .flatMap((c) => c.scenes)
                .find((s) => s.id === person.sceneId)
              return (
                <span key={person.id} className={person.sceneId === scene?.id ? 'presence__person is-here' : 'presence__person'}>
                  {person.displayName}
                  {where ? ` · ${person.sceneId === scene?.id ? 'here' : where.title}` : ''}
                </span>
              )
            })}
          </p>
        )}

        {savedElsewhere && scene && (
          <p className="access-note">
            {savedElsewhere} just saved this scene.{' '}
            <button
              type="button"
              className="linklike"
              onClick={() => {
                setSavedElsewhere(null)
                void openScene(scene)
              }}
            >
              Load their version
            </button>
            {' '}— or keep writing, and the next save asks which to keep.
          </p>
        )}

        {scene && !writable && (
          <p className="access-note">
            {can.role === 'reader'
              ? `You are reading ${outline.access?.owner.displayName ?? 'this'}’s draft. The text is read only.`
              : can.role === 'editor'
                ? 'You are on this book as an editor: select a passage and open Changes to propose an edit.'
                : 'This chapter is not one of yours. You can read it; the chapters you were handed are marked in the outline.'}
          </p>
        )}

        {writable && <Toolbar editor={editor} />}

        <GoalBar
          summary={summary}
          sessionWords={sessionWords}
          sceneWords={wordCount}
          sceneTarget={scene?.targetWords ?? null}
          onSetDailyGoal={setDailyGoal}
          onSetWeeklyGoal={setWeeklyGoal}
          bookWords={outline.wordCount}
          bookTarget={outline.targetWords ?? null}
          bookDeadline={outline.targetDate ?? null}
          canSetBookTarget={can.isOwner}
          onSetBookTarget={async (target, deadline) => {
            const work = await api.works.update(workId, {
              targetWords: target ?? 0,
              targetDate: deadline ?? '',
            })
            setOutline((current) => (current ? { ...current, ...work } : current))
          }}
          onSetSceneTarget={async (target) => {
            if (!scene) return
            const updated = await api.scenes.update(scene.id, { targetWords: target })
            setScene((current) => (current ? { ...current, targetWords: updated.targetWords } : current))
          }}
        />

        {view === 'corkboard' ? (
          <div className="prose-scroll">
            <Corkboard
              outline={outline}
              activeSceneId={scene?.id ?? null}
              onOpenScene={(target) => {
                setView('write')
                void openScene(target)
              }}
              onMove={async (moved, toChapterId, toIndex) => {
                // Moving between chapters is a scene update; reordering within
                // one is a reorder. Both end with the outline reloaded.
                const chapter = outline.chapters.find((c) => c.id === toChapterId)
                if (!chapter) return
                const remaining = chapter.scenes.filter((s) => s.id !== moved.id)
                const ids = [
                  ...remaining.slice(0, toIndex).map((s) => s.id),
                  moved.id,
                  ...remaining.slice(toIndex).map((s) => s.id),
                ]
                if (moved.chapterId !== toChapterId) {
                  await api.scenes.update(moved.id, { chapterId: toChapterId })
                }
                await api.scenes.reorder(toChapterId, ids)
                void loadOutline()
              }}
            />
          </div>
        ) : scene ? (
          // The scroll container is what typewriter scrolling drives; the
          // typography lands as custom properties on the prose itself.
          <div className="prose-scroll">
            <EditorContent
              editor={editor}
              className="prose prose--editable"
              style={proseStyle}
            />
          </div>
        ) : (
          <p className="empty">Add a scene on the left to start writing.</p>
        )}
      </section>

      {panel === 'prefs' && (
        <SidePanel title="Writing" onClose={closePanel}>
          <WritingPreferencesPanel
            preferences={preferences}
            onChange={setPreference}
            onReset={resetPreferences}
          />
        </SidePanel>
      )}

      {panel === 'details' && scene && (
        <SidePanel title={scene.title} onClose={closePanel}>
          <SceneDetails
            scene={scene}
            canMerge={!!nextScene}
            onSplit={splitHere}
            onMerge={mergeNext}
            onSaved={(updated) => {
              setScene((current) => (current ? { ...current, ...updated } : current))
              void loadOutline()
            }}
          />
        </SidePanel>
      )}

      {panel === 'changes' && scene && (
        <SidePanel title="Proposed changes" onClose={closePanel}>
          <ChangesPanel
            scene={scene}
            selection={selection}
            canPropose={can.canSuggest(scene.chapterId)}
            canResolve={writable}
            // The server edited the document, so the editor has to re-read it
            // rather than keep the copy it was holding.
            onApplied={() => {
              void openScene(scene)
              void loadOutline()
            }}
          />
        </SidePanel>
      )}

      {panel === 'bible' && (
        <SidePanel title="Story bible" onClose={closePanel}>
          <StoryBible workId={workId} scene={scene} />
        </SidePanel>
      )}

      {panel === 'tasks' && (
        <SidePanel title="To do" onClose={closePanel}>
          <TaskList
            workId={workId}
            sceneId={scene?.id ?? null}
            onOpenScene={(id) => {
              // The list keeps ids; the editor opens whole scenes, so look it
              // up in the outline rather than fetching a second copy.
              const target = outline.chapters
                .flatMap((chapter) => chapter.scenes)
                .find((candidate) => candidate.id === id)
              if (target) void openScene(target)
            }}
          />
        </SidePanel>
      )}

      {panel === 'front' && outline && (
        <SidePanel title="Front matter" onClose={closePanel}>
          <FrontMatter
            work={outline}
            onSaved={(work) =>
              setOutline((current) => (current ? { ...current, ...work } : current))
            }
          />
        </SidePanel>
      )}

      {panel === 'find' && (
        <SidePanel title="Find" onClose={closePanel}>
          <FindPanel
            workId={workId}
            onOpen={(id) => {
              const target = outline.chapters
                .flatMap((chapter) => chapter.scenes)
                .find((candidate) => candidate.id === id)
              if (target) void openScene(target)
            }}
          />
        </SidePanel>
      )}

      {panel === 'summary' && (
        <SidePanel title="Summarise" onClose={closePanel}>
          <SummaryPanel
            outline={outline}
            scene={scene}
            canUseSynopsis={can.isOwner}
            onApplied={() => {
              void loadOutline()
              if (scene) void api.scenes.get(scene.id).then((s) => setScene((c) => (c ? { ...c, summary: s.summary } : c)))
            }}
          />
        </SidePanel>
      )}

      {panel === 'people' && (
        <SidePanel title="People" onClose={closePanel}>
          <CollaboratorsPanel outline={outline} permissions={can} />
        </SidePanel>
      )}

      {panel === 'activity' && (
        <SidePanel title="Activity" onClose={closePanel}>
          <ActivityPanel workId={workId} refreshKey={savedAt} />
        </SidePanel>
      )}

      {panel === 'versions' && (
        <VersionPanel
          workId={workId}
          onClose={closePanel}
          versions={versions}
          onChanged={() => {
            void loadVersions()
            void loadOutline()
          }}
        />
      )}

      {panel === 'history' && scene && (
        <SidePanel title="This scene" onClose={closePanel}>
          <RevisionPanel
            sceneId={scene.id}
            refreshKey={savedAt}
            onRestored={() => {
              void openScene(scene)
              void loadOutline()
            }}
          />
        </SidePanel>
      )}
    </main>
  )
}
