import { useEffect } from 'react'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import { CommentAnchors } from '../editor/commentAnchors'
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
  onSelect: (selection: Selection | null) => void
  onActivateComment: (commentId: string) => void
}

/**
 * One scene of a published snapshot, rendered read-only.
 *
 * The reader is a real ProseMirror instance rather than static HTML, which is
 * what makes highlight-to-comment exact: the positions a reviewer selects are
 * the same positions the server stores, in the same document.
 */
export function ReaderScene({
  sceneId, title, content, anchors, activeCommentId, onSelect, onActivateComment,
}: Props) {
  const editor = useEditor(
    {
      editable: false,
      extensions: [StarterKit, Underline, CommentAnchors],
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
