import { useEffect, useMemo } from 'react'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import { Annotations, refreshAnnotations } from '../editor/annotations'
import type { AnnotationSpec } from '../editor/annotations'
import { CommentAnchors, place } from '../editor/commentAnchors'
import { Spellcheck, refreshSpelling } from '../editor/spelling/spellcheck'
import type { Speller } from '../editor/spelling/speller'
import { SelectionMenu, useSelectionMenu } from './SelectionMenu'
import type { MenuTarget, SelectionAction } from './SelectionMenu'
import { ReactionGutter } from '../editor/reactionGutter'
import type { ReactionKind } from '../editor/reactionGutter'
import type { AnchorSpec } from '../editor/commentAnchors'
import type { DocNode } from '../api/types'

const NO_ANNOTATIONS: AnnotationSpec[] = []

export interface Selection {
  sceneId: string
  from: number
  to: number
  quote: string
}

interface Props {
  sceneId: string
  title: string
  content: DocNode
  anchors: AnchorSpec[]
  activeCommentId: string | null
  /** What this reader has marked in this scene, by block index. */
  marks: Record<number, ReactionKind>
  onSelect: (selection: Selection | null) => void
  onActivateComment: (commentId: string) => void
  onReact: (sceneId: string, blockIndex: number, kind: ReactionKind) => void
  /** This reader's own highlights and bookmarks in this scene. */
  annotations?: AnnotationSpec[]
  /** What a right-click on selected text offers; none, and the browser's own menu stays. */
  menuActions?: (target: MenuTarget) => SelectionAction[]
  /** A passage to select and bring into view; a new `key` asks again. */
  jump?: { from: number; to: number; key: number } | null
  /** The notes from an earlier version whose passage is no longer in this scene. */
  onUnplaced?: (sceneId: string, commentIds: string[]) => void
  /** The checker that underlines misspelled words; none, and nothing is marked. */
  speller?: Speller | null
}

/**
 * One scene of a published snapshot, rendered read-only.
 *
 * The reader is a real ProseMirror instance rather than static HTML, which is
 * what makes highlight-to-comment exact: the positions a reviewer selects are
 * the same positions the server stores, in the same document.
 */
export function ReaderScene({
  sceneId, title, content, anchors, activeCommentId, marks, onSelect, onActivateComment, onReact,
  annotations = NO_ANNOTATIONS, menuActions, jump, onUnplaced, speller = null,
}: Props) {
  // Each scene gets its own instances. An extension declared at module level is
  // one object, and the page mounts a dozen editors: passing the bare
  // extensions would give every scene the same options, so the last one to
  // mount would own them — every gutter reporting the last scene's id, every
  // scene painting the last scene's comment anchors. `configure` clones.
  const extensions = useMemo(
    () => [
      StarterKit,
      Underline,
      CommentAnchors.configure({ anchors: [], activeId: null }),
      ReactionGutter.configure({ marks: {}, onReact: () => {} }),
      Annotations.configure({ annotations: [] }),
      Spellcheck.configure({ speller: null, enabled: false }),
    ],
    [],
  )

  const editor = useEditor(
    {
      editable: false,
      extensions,
      content: content as never,
      // The app checks spelling itself, and only when the reader asks.
      editorProps: { attributes: { spellcheck: 'false' } },
      onSelectionUpdate({ editor }) {
        const { from, to, empty } = editor.state.selection
        if (empty) {
          onSelect(null)
          return
        }
        onSelect({ sceneId, from, to, quote: editor.state.doc.textBetween(from, to, ' ') })
      },
    },
    [sceneId],
  )

  // Decorations are read off the extension's options, so pushing new anchors in
  // means mutating them and nudging the view to recompute.
  useEffect(() => {
    if (!editor) return
    const extension = editor.extensionManager.extensions.find((e) => e.name === 'commentAnchors')
    if (!extension) return
    extension.options.anchors = anchors
    extension.options.activeId = activeCommentId
    editor.view.dispatch(editor.state.tr)
  }, [editor, anchors, activeCommentId])

  useEffect(() => {
    if (!editor || !onUnplaced) return
    onUnplaced(sceneId, anchors.filter((a) => a.carried && !place(editor.state.doc, a)).map((a) => a.id))
  }, [editor, anchors, onUnplaced, sceneId])

  useEffect(() => {
    if (!editor) return
    const extension = editor.extensionManager.extensions.find((e) => e.name === 'reactionGutter')
    if (!extension) return
    extension.options.marks = marks
    extension.options.onReact = (blockIndex: number, kind: ReactionKind) =>
      onReact(sceneId, blockIndex, kind)
    editor.view.dispatch(editor.state.tr)
  }, [editor, marks, onReact, sceneId])

  useEffect(() => {
    if (!editor) return
    const extension = editor.extensionManager.extensions.find((e) => e.name === 'annotations')
    if (!extension) return
    extension.options.annotations = annotations
    editor.view.dispatch(refreshAnnotations(editor.state))
  }, [editor, annotations])

  // One checker for the whole page, handed to every scene; each repaints when
  // it is switched or has finished loading its dictionary.
  useEffect(() => {
    if (!editor) return
    const extension = editor.extensionManager.extensions.find((e) => e.name === 'spellcheck')
    if (!extension) return
    extension.options.speller = speller
    extension.options.enabled = !!speller
    const repaint = () => {
      if (!editor.isDestroyed) editor.view.dispatch(refreshSpelling(editor.state))
    }
    repaint()
    return speller?.subscribe(repaint)
  }, [editor, speller])

  useEffect(() => {
    if (!editor || !jump) return
    const size = editor.state.doc.content.size
    if (jump.to > size) return
    editor.chain().setTextSelection({ from: jump.from, to: jump.to }).scrollIntoView().run()
    // A read-only view does not scroll itself for a selection it was handed.
    const { node } = editor.view.domAtPos(jump.from)
    const element = node instanceof HTMLElement ? node : node.parentElement
    element?.scrollIntoView({ block: 'center', behavior: 'smooth' })
  }, [editor, jump])

  const contextMenu = useSelectionMenu(editor, !!menuActions)

  useEffect(() => {
    if (!editor) return
    const dom = editor.view.dom
    const onClick = (event: MouseEvent) => {
      const target = (event.target as HTMLElement).closest('[data-comment-id]')
      if (target) onActivateComment(target.getAttribute('data-comment-id')!)
    }
    dom.addEventListener('click', onClick)
    return () => dom.removeEventListener('click', onClick)
  }, [editor, onActivateComment])

  return (
    <article className="reader__scene" id={`scene-${sceneId}`}>
      <h3 className="reader__scene-title">{title}</h3>
      <EditorContent editor={editor} className="prose" />
      {contextMenu.target && menuActions && (
        <SelectionMenu
          target={contextMenu.target}
          actions={menuActions(contextMenu.target)}
          onClose={contextMenu.close}
        />
      )}
    </article>
  )
}
