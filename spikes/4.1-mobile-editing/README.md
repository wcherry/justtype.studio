# Spike: rich-text editing on iPhone (Phase 4.1)

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
| Code | `ios/…/Spike/AttributedDoc.swift` (256 lines) | `WebEditorView.swift` (104) + `editor/main.ts` (98) |
| Tests | `AttributedDocTests` (9) | `ProseMirrorContractTests` (2) |
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

**Not verified, and this is the open risk.** Everything iOS-specific about
typing in a web view: caret scroll-into-view above the keyboard, selection
handles and the magnifier, the input accessory view, and undo. None of it can
be tested from a command line, and it is exactly where a `WKWebView` editor
disappoints if it is going to. The host already does what can be done blind —
`keyboardDismissMode = .interactive`, and 40vh of bottom padding so the caret
has somewhere to sit — but a simulator pass is required before committing.

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
# Path A
cd ios/ScriptoriumKit && swift test --filter AttributedDocTests

# Path B — build the bundle, then serve it and read the positions back
cd web && npx vite build --config ../spikes/4.1-mobile-editing/editor/vite.config.ts
cd ../spikes/4.1-mobile-editing/editor/dist && python3 -m http.server 8777
# in the browser: window.scriptorium.load(doc); window.scriptorium.blocks()
cd ios/ScriptoriumKit && swift test --filter ProseMirrorContractTests
```
