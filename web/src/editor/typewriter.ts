import { Extension } from '@tiptap/core'
import { Plugin, PluginKey } from '@tiptap/pm/state'

const key = new PluginKey('typewriterScrolling')

/**
 * Keeps the caret at a fixed height in the viewport instead of letting it walk
 * down to the bottom edge.
 *
 * Driven from the scrolling element rather than `scrollIntoView`, which snaps to
 * the nearest edge and fights the effect. `behavior: 'auto'` deliberately: a
 * smooth scroll on every keystroke lags behind fast typing and reads as the text
 * sliding around underneath you.
 *
 * The measurement happens synchronously in `update`, which ProseMirror calls
 * after it has written the DOM — so the caret's position is already real and
 * there is nothing to wait for. An earlier version deferred to
 * `requestAnimationFrame` out of caution, which was not only unnecessary but
 * actively wrong in a backgrounded tab, where that callback never runs at all.
 */
export const TypewriterScrolling = Extension.create<{ enabled: boolean; anchor: number }>({
  name: 'typewriterScrolling',

  addOptions() {
    // 0.45 rather than 0.5: slightly above centre is where the eye naturally
    // rests, and it leaves the line being written room to breathe.
    return { enabled: false, anchor: 0.45 }
  },

  addProseMirrorPlugins() {
    const extension = this
    return [
      new Plugin({
        key,
        view(view) {
          const recentre = () => {
            if (!extension.options.enabled) return
            const scroller = view.dom.closest('.prose-scroll') as HTMLElement | null
            if (!scroller) return
            // A scroller with nothing to scroll would pin the first line to the
            // anchor and leave a gap above it.
            if (scroller.scrollHeight <= scroller.clientHeight) return

            const caret = view.coordsAtPos(view.state.selection.head)
            const box = scroller.getBoundingClientRect()
            const target = box.top + box.height * extension.options.anchor
            const delta = caret.top - target
            // Sub-pixel corrections are invisible and would fight the browser's
            // own rounding on every keystroke.
            if (Math.abs(delta) < 1) return
            scroller.scrollBy({ top: delta, behavior: 'auto' })
          }

          return {
            update(_view, previous) {
              const moved =
                previous.doc !== view.state.doc || !previous.selection.eq(view.state.selection)
              if (moved) recentre()
            },
          }
        },
      }),
    ]
  },
})
