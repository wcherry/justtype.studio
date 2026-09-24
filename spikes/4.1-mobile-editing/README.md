# Spike: rich-text editing on iPhone (Phase 4.1)

> **Acted on.** The embedded editor is the mobile editor now: the page here is
> the one that ships (`ios/ScriptoriumKit/Sources/ScriptoriumUI/Resources/`),
> and `WebEditorView` moved into the UI package. The attributed-string
> converter was deleted once it had answered the question — this file is what
> it left behind. What stays here is the build for the page and the record.

The mobile editor currently opens any scene carrying marks or non-paragraph
blocks **read-only**, because flattening an author's italics on save is worse
than not editing at all. Removing that limitation means choosing between two
paths. The roadmap says to decide with a spike rather than an argument, so both
were built far enough to answer with numbers.

**Recommendation: path B, the embedded editor.** The reasoning is below, and so
is the one thing that could still change it.

---

## What was built

| | Path A — `UITextView` | Path B — `WKWebView` + TipTap |
|---|---|---|
| Code | `AttributedDoc.swift` (256 lines, since deleted) | `WebEditorView.swift` (104) + `editor/main.ts` (98) |
| Tests | `AttributedDocTests` (9, since deleted) | `ProseMirrorContractTests` (2, kept) |
| Ships | nothing extra | 292 KB JS + 4 KB HTML (88 KB gzipped) in the app bundle |

Both are in the repository and both run. Neither is wired into the app.

## Path A: what an attributed string loses

The converter is real and its tests are the finding — each one names something a
writer did and asserts whether it survives.

**Survives:** headings with their level, bold/italic/underline in any
combination, links with their href, blockquotes, single-level lists, horizontal
rules, hard breaks, and the text exactly, astral characters included.

**Lost:**

- **Nested lists flatten.** A list inside a list comes back as one flat list.
- **Adjacent blockquotes merge.** Two quotes in a row become one with two
  paragraphs, because a flat string has nowhere to record where one ended.
- **Positions are not ProseMirror positions.** This is the serious one. A block
  boundary costs two positions in ProseMirror and one character in a string, so
  the two coordinate systems drift — by 2 after a single heading, and by **13**
  by the last paragraph of the test document. The drift follows *nesting*, not
  block count: every blockquote and list wrapper costs two more. So a mapping
  cannot be an offset, it has to walk the document, and every comment anchor
  written on a phone has to go through it.

**And a design cost that is not in the tests.** Marks are carried in an
attribute of our own, because a bold serif face and a regular face with the bold
trait are the same picture and different data. That means owning every
formatting command: the system's own ⌘B, the Format menu, and paste would all
write font traits this converter cannot see.

**What is not written, and would be.** The `UITextView` wrapper, typing-attribute
maintenance, list continuation on Return, the toolbar, and the position mapper.
The 256 lines are the converter alone.

## Path B: what the embedded editor costs

The document model *is* ProseMirror's, so there is nothing to lose in
translation — the question is what the web view costs instead.

**Verified:**

- The bundle builds from the web app's own TipTap install, and is one HTML file
  and one JS file: 296 KB on disk, loaded from the app bundle, no network. A
  writer on a plane can open a scene.
- Editing round-trips exactly. Selecting inside a blockquote and applying bold
  produced precisely the JSON the API takes, with the surrounding text split
  into the right runs and no other node touched.
- **Swift and ProseMirror agree on positions, checked against the real editor.**
  `ProseMirrorContractTests` compares `DocNode.blocks()` against numbers
  captured from `window.scriptorium.blocks()` in the built bundle. They match
  exactly, emoji included. That is the contract comments depend on, and it is
  now a test rather than a belief.
- The bridge is the whole 202 lines: a document in, documents and selections
  out. Nothing about the manuscript lives in the page.

## The simulator pass

Run since. `spikes/4.1-mobile-editing/UITests` is a UI test target driving the
real thing on an iPhone 17 Pro, reachable with `-spike41`; four of the five
questions now have answers.

| | |
|---|---|
| The document loads and renders | **yes** — heading, italic run and blockquote, all from the app bundle with no network |
| Typing reaches the document | **yes** — the word count moves in Swift as keys land in the page |
| A selection reports itself | **yes** — a long press sends `{from, to}` back across the bridge |
| Undo | **yes** — ⌘Z puts the words back; it drives the page's own undo stack |
| Caret above the keyboard | **unanswered** — see below |

**Three bugs, all of them in the bridge rather than the editor.**

1. `limitsNavigationsToAppBoundDomains = true` with no `WKAppBoundDomains` in
   the Info.plist refuses every navigation, file URLs included, and renders a
   blank white rectangle with nothing in the console. Removed — there is no
   remote code here to restrict.
2. A module script will not load over `file://`. Inlining it into the HTML is
   the fix, but an inline classic script in the `<head>` runs before `#editor`
   exists, so it has to go at the end of the body.
3. **Inlining is gone, and this is why it had to be.** It corrupted the bundle
   twice over. `$&`, `` $` `` and friends are interpreted in a replacement
   string, and minified JavaScript is full of dollars and backticks: one
   template literal's opening backtick was eaten a hundred kilobytes in. The
   symptom was a blank page and `SyntaxError: Unexpected identifier 'tiptap'`
   — a hundred lines from the actual damage. Passing a *function* as the
   replacement fixes it; `inline.mjs` says so in a comment, because the next
   person will reach for the string.

   The first was `String.replace` with a replacement *string*:

   `$&`, `` $` `` and friends are interpreted there, minified JavaScript is
   full of dollars and backticks, and one template literal lost its opening
   backtick a hundred kilobytes in. A replacement *function* fixed that.

   The second could not be fixed: wrapping 300 KB of minified JavaScript in a
   `try` block turns its top-level `const`s into block-scoped ones, and on
   **iOS 17** that is a temporal-dead-zone error — `Can't find variable: An` —
   from inside ProseMirror's plugin-key counter. The same bytes ran perfectly
   on iOS 26, which is the worst way to have a bug.

   The editor is now served under a custom scheme by a `WKURLSchemeHandler`
   (`EditorScheme.swift`). The page gets a real origin, ships exactly as Vite
   built it, module script and all, and there is no text surgery on minified
   code anywhere in the pipeline. That was listed here as the alternative; the
   iPad made it the choice.

**Still open: does the caret clear the keyboard?** The software keyboard never
presented for the web view in this environment. `app.keyboards` reports a
233-point element with 35 keys — parked at y=952 on an 874-point screen, with
only the input accessory bar visible in every screenshot. Measuring the caret
against a keyboard that is off the bottom of the screen proves nothing, so the
test throws `XCTSkip` with those numbers rather than banking a pass it did not
earn. Answering it needs a real device, or somebody with Simulator.app open to
turn off I/O ▸ Keyboard ▸ Connect Hardware Keyboard.

**One defect worth fixing in the implementation.** On first paint the heading
is missing: the accessibility tree has it, but it is not drawn until something
forces a relayout — tapping into the page brings it back. The document is
pushed on `ready`, which can land before the web view has laid out. A shipping
version should push after first layout, or force a repaint after `setContent`.


## Why B

Path A's losses are not the kind that get fixed by finishing it. Nested lists
and merged blockquotes are limits of a flat representation, and the position
drift means a second coordinate system to keep exactly in step with the server
and the browser forever — the same class of problem that comment anchoring was
designed to avoid by pointing at immutable snapshots.

Path B has one editor, one document model and one set of positions across all
three clients. A schema change happens once. `hasRichFormatting` and the
read-only guard go away entirely rather than being narrowed.

The cost is a web view inside a native app, and 296 KB. That is a real cost and
it buys the elimination of an entire class of divergence.

## If the simulator pass fails

If the keyboard and selection behaviour turn out unacceptable, the fallback is
not path A as built here. It is path A **restricted to what it does not lose**:
paragraphs, headings and inline marks, with scenes containing lists or nested
structures still opening read-only. That is a smaller promise than 4.1 makes,
and it should be written down as one rather than shipped as parity.

## Reproducing

```sh
# Path B — build the bundle, then serve it and read the positions back
cd web && npx vite build --config ../spikes/4.1-mobile-editing/editor/vite.config.ts
cd ../spikes/4.1-mobile-editing/editor/dist && python3 -m http.server 8777
# in the browser: window.scriptorium.load(doc); window.scriptorium.blocks()
cd ios/ScriptoriumKit && swift test --filter ProseMirrorContractTests

# The simulator pass. The built page ships as a package resource; rebuild with
#   cd web && npx vite build --config ../spikes/4.1-mobile-editing/editor/vite.config.ts
#   rm -rf ../ios/ScriptoriumKit/Sources/ScriptoriumUI/Resources/Editor
#   cp -R ../spikes/4.1-mobile-editing/editor/dist \
#     ../ios/ScriptoriumKit/Sources/ScriptoriumUI/Resources/Editor
cd ios && xcodegen generate
xcodebuild test -project Scriptorium.xcodeproj -scheme Scriptorium \
  -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
  -only-testing:ScriptoriumUITests

# Or just look at it:
xcrun simctl launch booted app.scriptorium.ios -spike41
```
