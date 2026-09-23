import { useCallback, useEffect, useState } from 'react'
import { useParams } from 'react-router-dom'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import Placeholder from '@tiptap/extension-placeholder'

import { api } from '../api/client'
import type { Outline as OutlineType, Scene, SceneWithContent, Version } from '../api/types'
import { ConflictBanner } from '../components/ConflictBanner'
import { Corkboard } from '../components/Corkboard'
import { FrontMatter } from '../components/FrontMatter'
import { GoalBar } from '../components/GoalBar'
import { Outline } from '../components/Outline'
import { SceneDetails } from '../components/SceneDetails'
import { StoryBible } from '../components/StoryBible'
import { RevisionPanel } from '../components/RevisionPanel'
import { Toolbar } from '../components/Toolbar'
import { WritingPreferencesPanel } from '../components/WritingPreferences'
import { VersionPanel } from '../components/VersionPanel'
import { FocusMode } from '../editor/focusMode'
import { useWritingPreferences } from '../editor/preferences'
import { ProseTypography } from '../editor/typography'
import { TypewriterScrolling } from '../editor/typewriter'
import { useAutosave } from '../editor/useAutosave'
import { useSessionGoals } from '../editor/useSessionGoals'
import { cache } from '../offline/cache'
import { pendingFor, startQueueReplay } from '../offline/queue'

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

export function EditorPage() {
  const { workId = '' } = useParams()
  const [outline, setOutline] = useState<OutlineType | null>(null)
  const [versions, setVersions] = useState<Version[]>([])
  const [scene, setScene] = useState<Scene | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [panel, setPanel] =
    useState<'none' | 'versions' | 'history' | 'prefs' | 'front' | 'details' | 'bible'>('none')
  const [view, setView] = useState<'write' | 'corkboard'>('write')
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

  const onSaved = useCallback((workWordCount: number) => {
    setSavedAt(Date.now())
    setOutline((current) => (current ? { ...current, wordCount: workWordCount } : current))
  }, [])

  const {
    state: saveState, wordCount, setWordCount, pending, conflict, schedule, flush, resolveWith,
  } = useAutosave({ scene, editor, onSaved })

  const { summary, sessionWords, refresh: refreshGoals, setDailyGoal } = useSessionGoals(
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
            {scene && (
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
            <button
              type="button"
              className={preferences.focusMode ? 'is-active' : undefined}
              title="Dim everything but the paragraph you are in"
              onClick={() => setPreference('focusMode', !preferences.focusMode)}
            >
              Focus
            </button>
            <button
              type="button"
              onClick={() => setPanel((p) => (p === 'prefs' ? 'none' : 'prefs'))}
            >
              Aa
            </button>
            <button
              type="button"
              className={view === 'corkboard' ? 'is-active' : undefined}
              onClick={() => setView((v) => (v === 'write' ? 'corkboard' : 'write'))}
            >
              {view === 'write' ? 'Corkboard' : 'Write'}
            </button>
            <button
              type="button"
              onClick={() => setPanel((p) => (p === 'details' ? 'none' : 'details'))}
            >
              Scene
            </button>
            <button
              type="button"
              onClick={() => setPanel((p) => (p === 'bible' ? 'none' : 'bible'))}
            >
              Bible
            </button>
            <button
              type="button"
              onClick={() => setPanel((p) => (p === 'front' ? 'none' : 'front'))}
            >
              Front matter
            </button>
            <button
              type="button"
              onClick={() => setPanel((p) => (p === 'history' ? 'none' : 'history'))}
            >
              History
            </button>
            <button
              type="button"
              onClick={() => setPanel((p) => (p === 'versions' ? 'none' : 'versions'))}
            >
              {panel === 'versions' ? 'Hide versions' : 'Versions'}
            </button>
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

        <Toolbar editor={editor} />

        <GoalBar
          summary={summary}
          sessionWords={sessionWords}
          sceneWords={wordCount}
          sceneTarget={scene?.targetWords ?? null}
          onSetDailyGoal={setDailyGoal}
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
        <aside className="versions">
          <header className="versions__header">
            <h3>Writing</h3>
          </header>
          <WritingPreferencesPanel
            preferences={preferences}
            onChange={setPreference}
            onReset={resetPreferences}
          />
        </aside>
      )}

      {panel === 'details' && scene && (
        <aside className="versions">
          <header className="versions__header">
            <h3>{scene.title}</h3>
          </header>
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
        </aside>
      )}

      {panel === 'bible' && (
        <aside className="versions">
          <header className="versions__header">
            <h3>Story bible</h3>
          </header>
          <StoryBible workId={workId} scene={scene} />
        </aside>
      )}

      {panel === 'front' && outline && (
        <aside className="versions">
          <header className="versions__header">
            <h3>Front matter</h3>
          </header>
          <FrontMatter
            work={outline}
            onSaved={(work) =>
              setOutline((current) => (current ? { ...current, ...work } : current))
            }
          />
        </aside>
      )}

      {panel === 'versions' && (
        <VersionPanel
          workId={workId}
          versions={versions}
          onChanged={() => {
            void loadVersions()
            void loadOutline()
          }}
        />
      )}

      {panel === 'history' && scene && (
        <aside className="versions">
          <header className="versions__header">
            <h3>This scene</h3>
          </header>
          <RevisionPanel
            sceneId={scene.id}
            refreshKey={savedAt}
            onRestored={() => {
              void openScene(scene)
              void loadOutline()
            }}
          />
        </aside>
      )}
    </main>
  )
}
