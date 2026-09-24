import SwiftUI
import ScriptoriumKit

/// **Spike — Phase 4.1.** The harness the simulator pass drives.
///
/// Reached only with `-spike41` on the command line, so the app it is bolted
/// to is otherwise untouched. Everything on screen is a readout: the questions
/// this pass exists to answer are about behaviour, and behaviour is only
/// visible if the state is.
struct SpikeEditorScreen: View {
    @State private var document = SpikeEditorScreen.sample
    @State private var words = 0
    @State private var selection = (from: 0, to: 0)
    @State private var marks: Set<String> = []
    /// Every document the page has sent back, so undo is observable.
    @State private var revisions = 0
    @State private var problem: String?

    var body: some View {
        VStack(spacing: 0) {
            WebEditorView(
                document: document,
                onChange: { doc, count in
                    document = doc
                    words = count
                    revisions += 1
                },
                onSelection: { from, to, active in
                    selection = (from, to)
                    marks = active
                },
                onProblem: { problem = $0 }
            )

            if let problem {
                Text(problem)
                    .font(.footnote)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.red.opacity(0.2))
                    .accessibilityIdentifier("spike.problem")
            }

            Divider()
            HStack(spacing: 14) {
                Text("\(words)w").accessibilityIdentifier("spike.words")
                Text("\(selection.from)–\(selection.to)").accessibilityIdentifier("spike.selection")
                Text(marks.sorted().joined(separator: "+"))
                    .accessibilityIdentifier("spike.marks")
                Text("\(revisions)").accessibilityIdentifier("spike.revisions")
                Spacer()
                Text(String(blocksSignature.prefix(40)))
                    .accessibilityIdentifier("spike.blocks")
            }
            .font(.caption.monospaced())
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(.thinMaterial)
        }
    }

    /// Types and positions of the top-level blocks, as Swift computes them.
    private var blocksSignature: String {
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
                    API.DocNode(
                        marks: [API.DocMark(type: "italic")],
                        text: "past",
                        type: "text"
                    ),
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
