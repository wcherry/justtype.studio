/**
 * Spike — Phase 4.1, path B. The editor a WKWebView would host.
 *
 * The point of this path is that there is one editor and one position model
 * for every client: this is the same TipTap the browser runs, so a document
 * cannot come back different and a position cannot mean something else.
 *
 * The bridge is deliberately small. Swift sends a document in and gets
 * documents and selections out; nothing about the manuscript lives in here.
 */
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import Underline from '@tiptap/extension-underline'

type Outbound =
  | { kind: 'ready' }
  | { kind: 'change'; doc: unknown; words: number }
  | { kind: 'selection'; from: number; to: number; marks: string[] }

declare global {
  interface Window {
    scriptorium: {
      load: (doc: unknown) => void
      toggle: (mark: string) => void
      blocks: () => { id: number; type: string; from: number; to: number; text: string }[]
    }
    webkit?: { messageHandlers?: { scriptorium?: { postMessage: (m: unknown) => void } } }
    /** Set by the harness when this runs in a browser rather than in the app. */
    __sent?: Outbound[]
  }
}

function send(message: Outbound) {
  const handler = window.webkit?.messageHandlers?.scriptorium
  if (handler) handler.postMessage(message)
  else (window.__sent ??= []).push(message)
}

const editor = new Editor({
  element: document.querySelector('#editor')!,
  extensions: [StarterKit, Underline],
  content: { type: 'doc', content: [{ type: 'paragraph' }] },
  onUpdate({ editor }) {
    send({
      kind: 'change',
      doc: editor.getJSON(),
      words: editor.state.doc.textBetween(0, editor.state.doc.content.size, ' ').split(/\s+/)
        .filter(Boolean).length,
    })
    // Toggling a mark changes the document without moving the selection, so
    // the toolbar would stay unlit until the writer tapped somewhere else.
    sendSelection(editor)
  },
  onSelectionUpdate({ editor }) {
    sendSelection(editor)
  },
})

function sendSelection(editor: Editor) {
  const { from, to } = editor.state.selection
  send({
    kind: 'selection',
    from,
    to,
    marks: ['bold', 'italic', 'underline', 'strike', 'code']
      .filter((m) => editor.isActive(m))
      .concat(editor.isActive('heading', { level: 2 }) ? ['h2'] : [])
      .concat(editor.isActive('blockquote') ? ['quote'] : [])
      .concat(editor.isActive('bulletList') ? ['bullet'] : []),
  })
}

window.scriptorium = {
  load: (doc) => editor.commands.setContent(doc as never, false),
  toggle: (mark) => {
    const chain = editor.chain().focus()
    switch (mark) {
      case 'bold': chain.toggleBold().run(); break
      case 'italic': chain.toggleItalic().run(); break
      case 'underline': chain.toggleUnderline().run(); break
      case 'strike': chain.toggleStrike().run(); break
      case 'h2': chain.toggleHeading({ level: 2 }).run(); break
      case 'quote': chain.toggleBlockquote().run(); break
      case 'bullet': chain.toggleBulletList().run(); break
    }
  },
  /**
   * The same shape `DocNode.blocks()` computes in Swift, straight from
   * ProseMirror. The spike's contract check: if these two ever disagree, a
   * comment anchored on a phone lands on the wrong words in a browser.
   */
  blocks: () => {
    const out: { id: number; type: string; from: number; to: number; text: string }[] = []
    editor.state.doc.forEach((node, offset, index) => {
      const isLeaf = node.type.name === 'horizontalRule' || node.isLeaf
      out.push({
        id: index,
        type: node.type.name,
        from: isLeaf ? offset : offset + 1,
        to: isLeaf ? offset + 1 : offset + 1 + node.content.size,
        text: node.textBetween(0, node.content.size, '\n'),
      })
    })
    return out
  },
}

send({ kind: 'ready' })
