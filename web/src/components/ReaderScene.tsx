import { useEffect, useMemo } from 'react'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import { CommentAnchors } from '../editor/commentAnchors'
import { ReactionGutter } from '../editor/reactionGutter'
import type { ReactionKind } from '../editor/reactionGutter'
import type { AnchorSpec } from '../editor/commentAnchors'
import type { DocNode } from '../api/types'

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
    ],
    [],
  )

  const editor = useEditor(
    {
      editable: false,
      extensions,
      content: content as never,
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
    </article>
  )
}
