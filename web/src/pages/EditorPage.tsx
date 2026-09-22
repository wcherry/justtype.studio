import { useCallback, useEffect, useState } from 'react'
import { useParams } from 'react-router-dom'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import Placeholder from '@tiptap/extension-placeholder'

import { api } from '../api/client'
import type { Outline as OutlineType, Scene, Version } from '../api/types'
import { Outline } from '../components/Outline'
import { Toolbar } from '../components/Toolbar'
import { VersionPanel } from '../components/VersionPanel'
import { useAutosave } from '../editor/useAutosave'

const SAVE_LABEL: Record<string, string> = {
  idle: '',
  dirty: 'Unsaved changes',
  saving: 'Saving…',
  saved: 'Saved',
  error: 'Save failed — retrying on next edit',
}

export function EditorPage() {
  const { workId = '' } = useParams()
  const [outline, setOutline] = useState<OutlineType | null>(null)
  const [versions, setVersions] = useState<Version[]>([])
  const [scene, setScene] = useState<Scene | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [showVersions, setShowVersions] = useState(false)

  const editor = useEditor({
    extensions: [
      StarterKit,
      Underline,
      Placeholder.configure({ placeholder: 'Begin where it is already happening…' }),
    ],
    content: '',
  })

  const { state: saveState, wordCount, setWordCount, schedule, flush } = useAutosave(
    scene?.id ?? null,
    editor,
  )

  const loadOutline = useCallback(async () => {
    try {
      const next = await api.works.outline(workId)
      setOutline(next)
      return next
    } catch (e) {
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
      const full = await api.scenes.get(target.id)
      setScene(full)
      setWordCount(full.wordCount)
      editor?.commands.setContent((full.content ?? { type: 'doc', content: [] }) as never, false)
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
            <span className={`save save--${saveState}`}>{SAVE_LABEL[saveState]}</span>
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
            <button type="button" onClick={() => setShowVersions((v) => !v)}>
              {showVersions ? 'Hide versions' : 'Versions'}
            </button>
          </div>
        </header>

        <Toolbar editor={editor} />

        {scene ? (
          <EditorContent editor={editor} className="prose prose--editable" />
        ) : (
          <p className="empty">Add a scene on the left to start writing.</p>
        )}
      </section>

      {showVersions && (
        <VersionPanel
          workId={workId}
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
