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
  | { kind: 'selection'; from: number; to: number; marks: string[]; quote: string }
  /** Reader mode: which page of how many, for the progress dots. */
  | { kind: 'pages'; page: number; pages: number }

declare global {
  interface Window {
    scriptorium: {
      load: (doc: unknown) => void
      toggle: (mark: string) => void
      blocks: () => { id: number; type: string; from: number; to: number; text: string }[]
      /** Reading rather than writing: no caret, no keyboard, still selectable. */
      setEditable: (editable: boolean) => void
      setTypography: (style: {
        size?: number
        family?: string
        leading?: number
        paged?: boolean
      }) => void
      turn: (by: number) => void
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
    // The words themselves, so a comment can quote what was selected without
    // the phone having to work out what lies between two positions.
    quote: from === to ? '' : editor.state.doc.textBetween(from, to, ' '),
    marks: ['bold', 'italic', 'underline', 'strike', 'code']
      .filter((m) => editor.isActive(m))
      .concat(editor.isActive('heading', { level: 2 }) ? ['h2'] : [])
      .concat(editor.isActive('blockquote') ? ['quote'] : [])
      .concat(editor.isActive('bulletList') ? ['bullet'] : []),
  })
}

window.scriptorium = {
  load: (doc) => {
    // `addToHistory: false` is the important part. Loading a scene is not an
    // edit, and leaving it on the undo stack means the first undo empties the
    // document — which autosave would then dutifully persist.
    editor
      .chain()
      .setMeta('addToHistory', false)
      .setContent(doc as never, false)
      // Start at the top. Setting content leaves the selection at the end of
      // the document, and the browser scrolls to wherever the selection is,
      // silently hiding the first paragraph of every scene a writer opens.
      .setTextSelection(0)
      .run()
    window.scrollTo(0, 0)
    const element = document.querySelector('.ProseMirror') as HTMLElement | null
    if (element) {
      element.scrollTop = 0
      element.scrollLeft = 0
    }
  },
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
      // A phone has no ⌘Z, and the shake gesture is a gamble with somebody's
      // paragraph. These are the buttons instead.
      case 'undo': chain.undo().run(); break
      case 'redo': chain.redo().run(); break
    }
  },
  setEditable: (editable) => editor.setEditable(editable),

  /**
   * Reading is not writing. The controls are the ones a reader reaches for —
   * size, face, leading — and paging, which is columns the width of the screen
   * with snapping, because a novel read in portrait on a phone is a stack of
   * pages rather than a scroll.
   */
  setTypography: ({ size, family, leading, paged }) => {
    const root = document.documentElement
    if (size) root.style.setProperty('--prose-size', `${size}px`)
    if (family) root.style.setProperty('--prose-family', family)
    if (leading) root.style.setProperty('--prose-leading', String(leading))
    if (paged !== undefined) {
      document.body.classList.toggle('paged', paged)
      reportPages()
    }
  },

  /** Next page, or the one before. Ignored when not paged. */
  turn: (by) => {
    const element = document.querySelector('.ProseMirror') as HTMLElement | null
    if (!element || !document.body.classList.contains('paged')) return
    element.scrollBy({ left: by * element.clientWidth, behavior: 'smooth' })
    window.setTimeout(reportPages, 350)
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

function reportPages() {
  const element = document.querySelector('.ProseMirror') as HTMLElement | null
  if (!element || !document.body.classList.contains('paged')) return
  const width = element.clientWidth || 1
  send({
    kind: 'pages',
    page: Math.round(element.scrollLeft / width) + 1,
    pages: Math.max(1, Math.ceil(element.scrollWidth / width)),
  })
}

window.addEventListener('resize', reportPages)
document.addEventListener('scroll', reportPages, { capture: true, passive: true })

send({ kind: 'ready' })
