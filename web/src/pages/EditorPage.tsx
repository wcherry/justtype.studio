import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
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
import { Notebook } from '../components/Notebook'
import { TaskList } from '../components/TaskList'
import { RevisionPanel } from '../components/RevisionPanel'
import { Icon } from '../components/Icon'
import { Menu } from '../components/Menu'
import type { MenuItem } from '../components/Menu'
import { GoalsPanel } from '../components/GoalsPanel'
import { MarksPanel } from '../components/MarksPanel'
import { SelectionMenu, useSelectionMenu } from '../components/SelectionMenu'
import type { SelectionAction } from '../components/SelectionMenu'
import { SidePanel } from '../components/SidePanel'
import { Tabs } from '../components/Tabs'
import { FORMAT_ACTIONS, ToolButton, Toolbar } from '../components/Toolbar'
import type { IconName } from '../components/Icon'
import { WritingPreferencesPanel } from '../components/WritingPreferences'
import { VersionPanel } from '../components/VersionPanel'
import { WritingRoomBar } from '../components/WritingRoomBar'
import { Annotations, MAX_MARK_QUOTE, currentRanges, findQuote, refreshAnnotations, tighten } from '../editor/annotations'
import type { AnnotationSpec } from '../editor/annotations'
import { useAnnotations } from '../editor/useAnnotations'
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
import { isUntitled, withPlaceholderNames } from '../naming'

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
  | 'prefs'
  | 'front'
  | 'notebook'
  /** The open scene: its details, proposed changes and history, as tabs. */
  | 'scene'
  | 'tasks'
  | 'people'
  | 'activity'
  | 'find'
  | 'summary'
  | 'marks'
  | 'goals'
  | 'about'

/** The panels used most while writing get a button of their own as well. */
const TOOLBAR_PANELS: { panel: Panel; icon: IconName; label: string }[] = [
  { panel: 'find', icon: 'find', label: 'Find in the book' },
  { panel: 'notebook', icon: 'notebook', label: 'Notebook: characters, places and continuity notes' },
  { panel: 'tasks', icon: 'tasks', label: 'To do' },
]

type SceneTab = 'details' | 'changes' | 'history'
const SCENE_TABS: { key: SceneTab; label: string }[] = [
  { key: 'details', label: 'Details' },
  { key: 'changes', label: 'Proposed changes' },
  { key: 'history', label: 'History' },
]

const SCENE_STATUSES: { value: Scene['status']; label: string }[] = [
  { value: 'outline', label: 'Outline' },
  { value: 'drafting', label: 'Drafting' },
  { value: 'revised', label: 'Revised' },
  { value: 'done', label: 'Done' },
]

/** Where the writer was in a book: the scene, and the selection within it. */
interface Resume { sceneId: string; from: number; to: number }
const resumeKey = (workId: string) => `justtype.resume.${workId}`
function readResume(workId: string): Resume | null {
  try {
    const value = JSON.parse(localStorage.getItem(resumeKey(workId)) ?? 'null')
    return value && typeof value.sceneId === 'string' && Number.isFinite(value.from) && Number.isFinite(value.to)
      ? value
      : null
  } catch {
    return null
  }
}
function writeResume(workId: string, resume: Resume) {
  try {
    localStorage.setItem(resumeKey(workId), JSON.stringify(resume))
  } catch {
    // Blocked site data: the writer just starts at the top next time.
  }
}

export function EditorPage() {
  const { workId = '' } = useParams()
  const [outline, setOutline] = useState<OutlineType | null>(null)
  const [versions, setVersions] = useState<VersionSummary[]>([])
  const [scene, setScene] = useState<Scene | null>(null)
  const [error, setError] = useState<string | null>(null)
  /** For showing only: untitled chapters and scenes called by their place. */
  const named = useMemo(() => (outline ? withPlaceholderNames(outline) : null), [outline])
  const [panel, setPanel] = useState<Panel>('none')
  const toggle = (which: Panel) => setPanel((p) => (p === which ? 'none' : which))
  const closePanel = () => setPanel('none')
  const [sceneTab, setSceneTab] = useState<SceneTab>('details')
  /** Opens the scene panel on one tab, or puts it away if it is already showing that one. */
  const toggleSceneTab = (tab: SceneTab) => {
    if (panel === 'scene' && sceneTab === tab) return setPanel('none')
    setSceneTab(tab)
    setPanel('scene')
  }
  /** The scene open now, for the selection handler to file the cursor under. */
  const sceneRef = useRef<string | null>(null)
  /** The To do panel's two lists: what is left to do, and comments on the text. */
  const [todoTab, setTodoTab] = useState<'todo' | 'comments'>('todo')
  const toggleTodoTab = (tab: 'todo' | 'comments') => {
    if (panel === 'tasks' && todoTab === tab) return setPanel('none')
    setTodoTab(tab)
    setPanel('tasks')
  }
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
      Annotations,
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
  // In the writing room the page is a share of the screen rather than a
  // number of characters, and the zoom scales the type within it.
  const roomStyle = {
    ...proseStyle,
    '--prose-size': `${(preferences.fontSize * preferences.roomZoom) / 100}px`,
    '--measure': `${preferences.roomWidth}%`,
  } as React.CSSProperties
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

  // The writer's own highlights and bookmarks: kept off the manuscript, and
  // found again whenever the text under them is replaced or saved.
  const marks = useAnnotations(workId ? { kind: 'draft', workId } : null)
  const [lostMarks, setLostMarks] = useState<Set<string>>(new Set())
  /** A mark to select once its scene is open. */
  const jumpTo = useRef<string | null>(null)
  useEffect(() => {
    if (!editor || !scene) return
    const doc = editor.state.doc
    const live = currentRanges(editor.state)
    const specs: AnnotationSpec[] = []
    const lost = new Set<string>()
    for (const mark of marks.list) {
      if (mark.sceneId !== scene.id) continue
      const size = doc.content.size
      const reads = (r: { from: number; to: number }) =>
        r.to <= size && r.from < r.to && doc.textBetween(r.from, r.to, ' ') === mark.quote
      const following = live.get(mark.id)
      const stored = { from: mark.fromPos, to: mark.toPos }
      // Where it was followed to while typing, then where it was stored, then
      // wherever the words are now; a mark whose words were edited follows
      // them rather than being lost.
      const found =
        (following && reads(following) && following) ||
        (reads(stored) && stored) ||
        findQuote(doc, mark.quote, mark.fromPos) ||
        following ||
        null
      const range = found && tighten(doc, found)
      if (!range) {
        lost.add(mark.id)
        continue
      }
      specs.push({ id: mark.id, kind: mark.kind, ...range })
      const quote = doc.textBetween(range.from, range.to, ' ')
      if (range.from !== mark.fromPos || range.to !== mark.toPos || quote !== mark.quote) {
        void marks.update(mark.id, { from: range.from, to: range.to, quote }).catch(() => undefined)
      }
    }
    setLostMarks((previous) => {
      const next = new Set([...previous].filter((id) => !marks.list.some((m) => m.id === id && m.sceneId === scene.id)))
      lost.forEach((id) => next.add(id))
      return next
    })
    const extension = editor.extensionManager.extensions.find((e) => e.name === 'annotations')
    if (!extension) return
    extension.options.annotations = specs
    editor.view.dispatch(refreshAnnotations(editor.state))

    const wanted = specs.find((spec) => spec.id === jumpTo.current)
    if (wanted) {
      jumpTo.current = null
      selectMark(wanted)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, scene?.id, scene?.contentHash, savedAt, marks.list])

  /** Selects a mark's passage and hands the editor the focus, so it shows. */
  const selectMark = (range: { from: number; to: number }) => {
    if (!editor) return
    editor.chain().setTextSelection(range).scrollIntoView().run()
    // The click that asked for it left focus on the panel's button; take it
    // once that click has finished.
    setTimeout(() => editor.view.focus(), 0)
  }

  // Right-click (or ⌘-click) on selected text, or on a mark.
  const contextMenu = useSelectionMenu(editor)
  const [markError, setMarkError] = useState<string | null>(null)
  const selectionActions = (): SelectionAction[] => {
    const target = contextMenu.target
    if (!target || !scene || !editor) return []
    const actions: SelectionAction[] = []
    const mark = target.annotationId ? marks.list.find((m) => m.id === target.annotationId) : null
    const range = target.selection && tighten(editor.state.doc, target.selection)
    const markAs = async (kind: 'highlight' | 'bookmark') => {
      if (!range) return
      const quote = editor.state.doc.textBetween(range.from, range.to, ' ')
      if (quote.length > MAX_MARK_QUOTE) {
        setMarkError(`A mark holds ${MAX_MARK_QUOTE.toLocaleString()} characters at most — select less.`)
        return
      }
      setMarkError(null)
      try {
        await marks.add({ sceneId: scene.id, kind, ...range, quote })
      } catch (e) {
        setMarkError(e instanceof Error ? e.message : 'Could not mark that')
      }
    }
    if (range) {
      actions.push({ key: 'highlight', label: 'Highlight', onSelect: () => void markAs('highlight') })
      actions.push({ key: 'bookmark', label: 'Bookmark', onSelect: () => void markAs('bookmark') })
      if (can.canSuggest(scene.chapterId)) {
        actions.push({
          key: 'comment',
          label: 'Comment…',
          compose: {
            placeholder: 'Your comment on this passage',
            submit: 'Comment',
            onSubmit: async (body) => {
              const quote = editor.state.doc.textBetween(range.from, range.to, ' ')
              if (quote.length > MAX_MARK_QUOTE) {
                setMarkError(`A comment holds ${MAX_MARK_QUOTE.toLocaleString()} characters of text at most — select less.`)
                return
              }
              setMarkError(null)
              try {
                await marks.add({ sceneId: scene.id, kind: 'comment', ...range, quote, note: body.trim().slice(0, 500) })
                { setTodoTab('comments'); setPanel('tasks') }
              } catch (e) {
                setMarkError(e instanceof Error ? e.message : 'Could not add that comment')
              }
            },
          },
        })
      }
      actions.push({
        key: 'copy',
        label: 'Copy',
        hint: '⌘C',
        onSelect: () => void navigator.clipboard?.writeText(target.selection!.quote),
      })
    }
    if (mark) {
      actions.push({
        key: 'remove',
        label: mark.kind === 'highlight' ? 'Remove highlight' : mark.kind === 'comment' ? 'Remove comment' : 'Remove bookmark',
        onSelect: () => void marks.remove(mark.id).catch((e) => setMarkError(e instanceof Error ? e.message : 'Could not remove that')),
      })
    }
    return actions
  }

  /** Opens a mark's scene, if it is not this one, and selects its passage. */
  const openMark = (id: string) => {
    const mark = marks.list.find((m) => m.id === id)
    if (!mark || !outline) return
    if (lostMarks.has(id)) {
      // Its words are gone from the draft: go to where it last was.
      const land = () => {
        if (!editor) return
        const at = Math.max(1, Math.min(mark.fromPos, editor.state.doc.content.size - 1))
        editor.chain().setTextSelection(at).scrollIntoView().run()
        setTimeout(() => editor.view.focus(), 0)
      }
      if (mark.sceneId === scene?.id) return land()
      const home = outline.chapters.flatMap((c) => c.scenes).find((s) => s.id === mark.sceneId)
      if (home) void openScene(home).then(land)
      return
    }
    jumpTo.current = id
    if (mark.sceneId === scene?.id) {
      // Already here: repaint, which selects it.
      editor?.view.dispatch(refreshAnnotations(editor.state))
      const range = currentRanges(editor!.state).get(id)
      if (range) {
        jumpTo.current = null
        selectMark(range)
      }
      return
    }
    const target = outline.chapters.flatMap((c) => c.scenes).find((s) => s.id === mark.sceneId)
    if (target) void openScene(target)
  }

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
      const scenes = next?.chapters.flatMap((c) => c.scenes) ?? []
      // Where the writer left off, if that scene is still in the book.
      const left = readResume(workId)
      const first = scenes.find((s) => s.id === left?.sceneId) ?? scenes[0]
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
      const resume = readResume(workId)
      const queued = await pendingFor(target.id)
      const content = queued?.content ?? full.content ?? { type: 'doc', content: [] }
      sceneRef.current = full.id
      setScene(full)
      setWordCount(full.wordCount)
      editor?.commands.setContent(content as never, false)
      // Back in the scene they left, at the words they left off at.
      if (editor && resume && resume.sceneId === full.id) {
        const size = editor.state.doc.content.size
        if (resume.to <= size) {
          editor.chain().setTextSelection({ from: resume.from, to: resume.to }).scrollIntoView().run()
        }
      }
    },
    [editor, flush, saveState, setWordCount, workId],
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
      if (sceneRef.current) writeResume(workId, { sceneId: sceneRef.current, from, to })
      setSelection(
        empty ? null : { from, to, quote: editor.state.doc.textBetween(from, to, ' ') },
      )
    }
    editor.on('selectionUpdate', handler)
    return () => {
      editor.off('selectionUpdate', handler)
    }
  }, [editor, workId])

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

  const setStatus = async (status: Scene['status']) => {
    if (!scene) return
    await api.scenes.update(scene.id, { status })
    setScene({ ...scene, status })
    void loadOutline()
  }

  /** Sets or clears goals; whatever is left out of `goals` stays as it is. */
  const saveGoals = async (goals: {
    daily?: number | null
    weekly?: number | null
    scene?: number | null
    book?: { target: number | null; deadline: string | null }
  }) => {
    if (goals.daily !== undefined) await setDailyGoal(goals.daily)
    if (goals.weekly !== undefined) await setWeeklyGoal(goals.weekly)
    if (goals.scene !== undefined && scene) {
      const updated = await api.scenes.update(scene.id, { targetWords: goals.scene })
      setScene((current) => (current ? { ...current, targetWords: updated.targetWords } : current))
    }
    if (goals.book && can.isOwner) {
      const work = await api.works.update(workId, {
        targetWords: goals.book.target ?? 0,
        targetDate: goals.book.deadline ?? '',
      })
      setOutline((current) => (current ? { ...current, ...work } : current))
    }
  }

  /** A side panel as a menu item: ticked while it is open. */
  const panelItem = (which: Panel, label: string, extra: Partial<MenuItem> = {}): MenuItem => ({
    key: which, label, checked: panel === which, onSelect: () => toggle(which), ...extra,
  })
  const format = (run: (e: NonNullable<typeof editor>) => void) => () => {
    if (!editor) return
    run(editor)
    // The closing menu hands focus back to its button; take it back for the
    // page once it has, so typing carries on.
    setTimeout(() => editor.view.focus(), 0)
  }
  const mainMenu: MenuItem[] = [
    {
      key: 'scene',
      label: 'Scene',
      items: [
        ...SCENE_TABS.map(({ key, label }) => ({
          key,
          label,
          disabled: !scene,
          checked: panel === 'scene' && sceneTab === key,
          onSelect: () => toggleSceneTab(key),
        })),
        ...(summariesOn && can.canWrite(scene?.chapterId ?? '') ? [panelItem('summary', 'Summarise…')] : []),
        {
          key: 'status',
          label: 'Status',
          divider: true,
          disabled: !scene || !writable,
          items: SCENE_STATUSES.map((s) => ({
            key: s.value, label: s.label, checked: scene?.status === s.value, onSelect: () => void setStatus(s.value),
          })),
        },
        { key: 'split', label: 'Split at the cursor', disabled: !scene || !writable, onSelect: () => void splitHere() },
        { key: 'merge', label: 'Merge with the next scene', disabled: !nextScene || !writable, onSelect: () => void mergeNext() },
      ],
    },
    {
      key: 'book',
      label: 'Book',
      items: [
        panelItem('find', 'Find'),
        panelItem('notebook', 'Notebook'),
        panelItem('tasks', 'To do', {
          checked: panel === 'tasks' && todoTab === 'todo',
          onSelect: () => toggleTodoTab('todo'),
        }),
        {
          key: 'comments', label: 'Comments', checked: panel === 'tasks' && todoTab === 'comments',
          onSelect: () => toggleTodoTab('comments'),
        },
        panelItem('marks', 'Highlights & bookmarks'),
        ...(can.isOwner
          ? [panelItem('front', 'Front matter', { divider: true }), panelItem('versions', 'Versions')]
          : []),
      ],
    },
    {
      key: 'format',
      label: 'Format',
      disabled: !writable,
      items: [
        ...FORMAT_ACTIONS.map((action, index) => ({
          key: action.title,
          label: action.title.replace(/ \(.*\)$/, ''),
          hint: action.title.match(/\((.*)\)$/)?.[1],
          divider: index === 4 || index === 8,
          checked: action.title === 'Scene break' ? undefined : !!editor && action.isActive(editor),
          onSelect: format(action.run),
        })),
        { key: 'undo', label: 'Undo', hint: '⌘Z', divider: true, onSelect: format((e) => e.chain().focus().undo().run()) },
        { key: 'redo', label: 'Redo', hint: '⇧⌘Z', onSelect: format((e) => e.chain().focus().redo().run()) },
      ],
    },
    {
      key: 'view',
      label: 'View',
      items: [
        { key: 'write', label: 'Write', checked: view === 'write', onSelect: () => setView('write') },
        { key: 'corkboard', label: 'Corkboard', checked: view === 'corkboard', onSelect: () => setView('corkboard') },
        { key: 'room', label: 'Writing room', hint: '⌘⇧F', divider: true, onSelect: page.enter },
        {
          key: 'focus', label: 'Focus on the paragraph', checked: preferences.focusMode,
          onSelect: () => setPreference('focusMode', !preferences.focusMode),
        },
        {
          key: 'typewriter', label: 'Typewriter scrolling', checked: preferences.typewriter,
          onSelect: () => setPreference('typewriter', !preferences.typewriter),
        },
        panelItem('prefs', 'Type, size and spacing…', { divider: true }),
      ],
    },
    {
      key: 'progress',
      label: 'Progress',
      items: [
        panelItem('goals', 'Set or reset goals…'),
        { key: 'reset', label: 'Reset all goals', onSelect: () => void saveGoals({ daily: null, weekly: null, scene: null, book: { target: null, deadline: null } }) },
      ],
    },
    {
      key: 'people',
      label: 'People',
      items: [panelItem('people', 'Who is on this book'), panelItem('activity', 'Activity')],
    },
    {
      key: 'help',
      label: 'Help',
      items: [panelItem('about', 'About Just Type Studio')],
    },
  ]

  if (error) return <main className="editor"><p className="error">{error}</p></main>
  if (!outline) return <main className="editor"><p className="empty">Opening…</p></main>

  /** A scene's title, or "Scene 2" for where it sits when it has none. */
  const nameOf = (sceneId: string) =>
    (named ?? outline).chapters.flatMap((c) => c.scenes).find((s) => s.id === sceneId)?.title ?? ''

  // The same outline in the sidebar and in the writing room's popup.
  const outlineProps: React.ComponentProps<typeof Outline> = {
    outline,
    activeSceneId: scene?.id ?? null,
    onSelectScene: openScene,
    onAddChapter: async () => {
      await api.chapters.create(workId, '')
      void loadOutline()
    },
    onAddScene: async (chapterId) => {
      const created = await api.scenes.create(chapterId, '')
      await loadOutline()
      void openScene(created)
    },
    onMoveScene: async (chapterId, ids) => {
      await api.scenes.reorder(chapterId, ids)
      void loadOutline()
    },
    onRenameScene: async (sceneId, title) => {
      await api.scenes.update(sceneId, { title })
      void loadOutline()
    },
    onRenameChapter: async (chapterId, title) => {
      await api.chapters.update(chapterId, { title })
      void loadOutline()
    },
    canWrite: can.canWrite,
    canShape: can.canShape,
  }

  return (
    <main className="editor">
      {page.on && (
        <>
          <WritingRoomBar
            outline={named ?? outline}
            scene={scene}
            wordCount={wordCount}
            saveLabel={SAVE_LABEL[saveState]}
            zoom={preferences.roomZoom}
            onZoom={(percent) => setPreference('roomZoom', percent)}
            width={preferences.roomWidth}
            onWidth={(percent) => setPreference('roomWidth', percent)}
            outlineShown={page.outline}
            onToggleOutline={page.toggleOutline}
            outlinePanel={
              <Outline
                {...outlineProps}
                className="outline--popup"
                // Choosing a scene puts the outline away and goes back to the
                // words; adding or moving scenes leaves it open.
                onSelectScene={(target) => {
                  page.toggleOutline()
                  void openScene(target).then(() => setTimeout(() => editor?.view.focus(), 0))
                }}
              />
            }
            onOpenScene={(target) => {
              void openScene(target).then(() => setTimeout(() => editor?.view.focus(), 0))
            }}
            onLeave={page.leave}
          />
          {/* Quiet enough to ignore, there for a glance. */}
          <div className={`page-status save--${saveState}`} aria-live="polite">
            {wordCount.toLocaleString()} · {SAVE_LABEL[saveState] || 'Saved'}
          </div>
        </>
      )}
      <Outline {...outlineProps} />

      <section className="editor__main">
        <header className="editor__header">
          <div className="editor__heading">
            <Menu
              label={<Icon name="menu" />}
              ariaLabel="Menu"
              tip="Menu: everything, by where it belongs"
              caret={false}
              align="left"
              className="editor__menu"
              active={panel !== 'none' && !TOOLBAR_PANELS.some((t) => t.panel === panel)}
              items={mainMenu}
            />
            <div className="editor__title">
            <h1 className={scene && isUntitled(scene.title) ? 'is-placeholder' : undefined}>
              {scene ? nameOf(scene.id) : outline.title}
            </h1>
            <p className="editor__meta">
              {scene && `${SCENE_STATUSES.find((s) => s.value === scene.status)?.label} · `}
              {wordCount.toLocaleString()} words in this scene · {outline.wordCount.toLocaleString()} in the work
            </p>
            </div>
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
              const where = (named ?? outline).chapters
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
                ? 'You are on this book as an editor: select a passage, then ⓘ → Proposed changes to propose an edit.'
                : 'This chapter is not one of yours. You can read it; the chapters you were handed are marked in the outline.'}
          </p>
        )}

        <Toolbar editor={editor} canFormat={writable && view === 'write'}>
          {TOOLBAR_PANELS.map(({ panel: which, icon, label }) => (
            <ToolButton
              key={which}
              icon={icon}
              label={label}
              pressed={panel === which}
              onClick={() => toggle(which)}
            />
          ))}
          <ToolButton icon="room" label="Writing room: nothing but the words (⌘⇧F)" onClick={page.enter} />
          <ToolButton
            icon="info"
            label={scene ? 'About this scene: details, proposed changes, history' : 'Open a scene to see about it'}
            pressed={panel === 'scene'}
            disabled={!scene}
            onClick={() => toggle('scene')}
          />
        </Toolbar>

        {markError && (
          <p className="access-note" role="alert">
            {markError}{' '}
            <button type="button" className="linklike" onClick={() => setMarkError(null)}>Dismiss</button>
          </p>
        )}
        {contextMenu.target && (
          <SelectionMenu target={contextMenu.target} actions={selectionActions()} onClose={contextMenu.close} />
        )}

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
              outline={named ?? outline}
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
              style={page.on ? roomStyle : proseStyle}
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

      {panel === 'scene' && scene && (
        <SidePanel title={nameOf(scene.id)} onClose={closePanel}>
          <Tabs label="About this scene" tabs={SCENE_TABS} active={sceneTab} onChange={setSceneTab}>
            {sceneTab === 'details' && (
              <SceneDetails
                scene={scene}
                canGenerate={summariesOn && writable}
                beforeGenerate={() => flush(true)}
                canMerge={!!nextScene}
                onSplit={splitHere}
                onMerge={mergeNext}
                onSaved={(updated) => {
                  setScene((current) => (current ? { ...current, ...updated } : current))
                  void loadOutline()
                }}
              />
            )}
            {sceneTab === 'changes' && (
              <ChangesPanel
                scene={scene}
                selection={selection}
                canPropose={can.canSuggest(scene.chapterId)}
                canResolve={writable}
                // The server edited the document, so the editor has to re-read
                // it rather than keep the copy it was holding.
                onApplied={() => {
                  void openScene(scene)
                  void loadOutline()
                }}
              />
            )}
            {sceneTab === 'history' && (
              <RevisionPanel
                sceneId={scene.id}
                refreshKey={savedAt}
                onRestored={() => {
                  void openScene(scene)
                  void loadOutline()
                }}
              />
            )}
          </Tabs>
        </SidePanel>
      )}

      {panel === 'notebook' && (
        <SidePanel title="Notebook" onClose={closePanel}>
          <Notebook workId={workId} scene={scene} />
        </SidePanel>
      )}

      {panel === 'tasks' && (
        <SidePanel title="To do" onClose={closePanel}>
          <Tabs
            label="To do and comments"
            tabs={[{ key: 'todo', label: 'To do' }, { key: 'comments', label: 'Comments' }]}
            active={todoTab}
            onChange={setTodoTab}
          >
            {todoTab === 'todo' ? (
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
            ) : (
              <MarksPanel
                comments
                marks={marks.list}
                scenes={(named ?? outline).chapters.flatMap((c) => c.scenes)}
                lost={lostMarks}
                onOpen={openMark}
                onRemove={(id) => void marks.remove(id)}
                onRename={(id, note) => marks.update(id, { note })}
              />
            )}
          </Tabs>
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
            nameOf={nameOf}
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

      {panel === 'marks' && (
        <SidePanel title="Highlights & bookmarks" onClose={closePanel}>
          <MarksPanel
            marks={marks.list}
            scenes={(named ?? outline).chapters.flatMap((c) => c.scenes)}
            lost={lostMarks}
            onOpen={openMark}
            onRemove={(id) => void marks.remove(id)}
            onRename={(id, note) => marks.update(id, { note })}
          />
        </SidePanel>
      )}

      {panel === 'goals' && (
        <SidePanel title="Goals" onClose={closePanel}>
          <GoalsPanel
            // Fresh fields whenever the goals underneath them change.
            key={`${summary?.dailyWordGoal}|${summary?.weeklyWordGoal}|${scene?.id}|${scene?.targetWords}|${outline.targetWords}|${outline.targetDate}`}
            daily={summary?.dailyWordGoal ?? null}
            weekly={summary?.weeklyWordGoal ?? null}
            scene={scene ? { target: scene.targetWords ?? null } : null}
            book={can.isOwner ? { target: outline.targetWords ?? null, deadline: outline.targetDate ?? null } : null}
            onSave={saveGoals}
          />
        </SidePanel>
      )}

      {panel === 'about' && (
        <SidePanel title="About" onClose={closePanel}>
          <div className="about">
            <h3>Just Type Studio</h3>
            <p>Version {__APP_VERSION__}</p>
            <p className="field__hint">A writing tool for long-form fiction, and a review surface for the readers you choose.</p>
          </div>
        </SidePanel>
      )}

      {panel === 'people' && (
        <SidePanel title="People" onClose={closePanel}>
          <CollaboratorsPanel outline={named ?? outline} permissions={can} />
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

    </main>
  )
}
