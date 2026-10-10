import { useEffect } from 'react'
import { EditorContent, useEditor } from '@tiptap/react'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'
import type { DocNode } from '../api/types'
import { useWritingPreferences } from '../editor/preferences'

/**
 * An answer, read-only, in the reader's own typography: the same face, size,
 * leading and measure they chose for their own writing.
 */
export function EntryText({ content }: { content: DocNode }) {
  const { style } = useWritingPreferences()
  const editor = useEditor({
    extensions: [StarterKit, Underline],
    content: content as never,
    editable: false,
  })
  useEffect(() => {
    if (editor && !editor.isDestroyed) editor.commands.setContent(content as never, false)
  }, [editor, content])
  return <EditorContent editor={editor} className="prose prose--reading" style={style} />
}
