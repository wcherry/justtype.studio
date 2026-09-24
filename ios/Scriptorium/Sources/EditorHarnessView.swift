import ScriptoriumKit
import ScriptoriumUI
import SwiftUI

/// A UI-test fixture: the real editor, a sample document, and no network.
///
/// Reached only with `-uitest`. It exists because the questions the UI tests
/// ask — does the keyboard come up, does typing reach Swift, does undo work —
/// are about the editing surface rather than about signing in, and a test that
/// has to log in first fails for reasons that have nothing to do with what it
/// is testing.
struct EditorHarnessView: View {
    @State private var document = EditorHarnessView.sample
    @State private var words = 0
    @State private var selection = (from: 0, to: 0)
    @State private var marks: Set<String> = []
    @State private var command: WebEditorView.Command?
    @State private var issued = 0
    @State private var problem: String?

    var body: some View {
        VStack(spacing: 0) {
            if let problem {
                Text(problem)
                    .font(.footnote)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.red.opacity(0.2))
                    .accessibilityIdentifier("harness.problem")
            }

            WebEditorView(
                document: document,
                command: command,
                onChange: { doc, count in
                    document = doc
                    words = count
                },
                onSelection: { from, to, active in
                    selection = (from, to)
                    marks = active
                },
                onProblem: { problem = $0 }
            )

            HStack(spacing: 8) {
                ForEach(["bold", "italic", "h2"], id: \.self) { name in
                    Button(name) {
                        issued += 1
                        command = WebEditorView.Command(name: name, issue: issued)
                    }
                    .accessibilityIdentifier("harness.\(name)")
                }
            }
            .font(.caption)
            .padding(.vertical, 6)

            Divider()
            HStack(spacing: 14) {
                Text("\(words)w").accessibilityIdentifier("harness.words")
                Text("\(selection.from)–\(selection.to)").accessibilityIdentifier("harness.selection")
                Text(marks.sorted().joined(separator: "+")).accessibilityIdentifier("harness.marks")
                Spacer()
                Text(documentMarks).accessibilityIdentifier("harness.docmarks")
                Text(String(signature.prefix(30))).accessibilityIdentifier("harness.blocks")
            }
            .font(.caption.monospaced())
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.thinMaterial)
        }
    }

    /// Every mark in the document Swift is holding. The point of the editor is
    /// that this is a real ProseMirror document, so this is the readout that
    /// proves a toolbar tap reached it rather than only lighting a button.
    private var documentMarks: String {
        func walk(_ node: API.DocNode, into found: inout Set<String>) {
            for mark in node.marks ?? [] { found.insert(mark.type) }
            for child in node.content ?? [] { walk(child, into: &found) }
        }
        var found: Set<String> = []
        walk(document, into: &found)
        return found.sorted().joined(separator: "+")
    }

    /// Types and positions of the top-level blocks, as Swift computes them.
    private var signature: String {
        document.blocks().map { "\($0.type):\($0.from)-\($0.to)" }.joined(separator: " ")
    }

    static let sample = API.DocNode(
        content: [
            API.DocNode(
                attrs: .object(["level": .number(2)]),
                content: [API.DocNode(text: "The Long Field", type: "text")],
                type: "heading"
            ),
            API.DocNode(
                content: [
                    API.DocNode(text: "The field went on ", type: "text"),
                    API.DocNode(marks: [API.DocMark(type: "italic")], text: "past", type: "text"),
                    API.DocNode(text: " where the light gave out.", type: "text"),
                ],
                type: "paragraph"
            ),
            API.DocNode(
                content: [
                    API.DocNode(
                        content: [
                            API.DocNode(
                                text: "She counted fenceposts until the counting stopped meaning anything.",
                                type: "text"
                            )
                        ],
                        type: "paragraph"
                    )
                ],
                type: "blockquote"
            ),
        ],
        type: "doc"
    )
}
