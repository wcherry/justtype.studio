import XCTest
@testable import ScriptoriumKit

/// **Spike — Phase 4.1, path B.** The position contract, checked against
/// ProseMirror itself rather than against my reading of it.
///
/// The fixture below was not written by hand. It is what the real editor
/// reported for this exact document, captured by loading
/// `spikes/4.1-mobile-editing/editor` in a browser and calling
/// `window.scriptorium.blocks()`. Regenerate it the same way if the document
/// or the schema changes.
///
/// If Swift and ProseMirror ever disagree here, a comment anchored on a phone
/// highlights the wrong words in a browser — which is the whole reason
/// `DocNode` counts in UTF-16 in the first place.
final class ProseMirrorContractTests: XCTestCase {
    private struct Block: Equatable {
        let id: Int
        let type: String
        let from: Int
        let to: Int
    }

    /// Straight from `window.scriptorium.blocks()` in the built editor.
    private let fromProseMirror: [Block] = [
        Block(id: 0, type: "heading", from: 1, to: 15),
        Block(id: 1, type: "paragraph", from: 17, to: 80),
        Block(id: 2, type: "blockquote", from: 82, to: 107),
        Block(id: 3, type: "bulletList", from: 109, to: 128),
        Block(id: 4, type: "horizontalRule", from: 129, to: 130),
        Block(id: 5, type: "paragraph", from: 131, to: 166),
    ]

    private func document() -> API.DocNode {
        func text(_ value: String, _ marks: [String] = []) -> API.DocNode {
            API.DocNode(
                marks: marks.isEmpty ? nil : marks.map { API.DocMark(type: $0) },
                text: value,
                type: "text"
            )
        }
        func paragraph(_ children: [API.DocNode]) -> API.DocNode {
            API.DocNode(content: children, type: "paragraph")
        }
        return API.DocNode(
            content: [
                API.DocNode(
                    attrs: .object(["level": .number(2)]),
                    content: [text("The Long Field")],
                    type: "heading"
                ),
                paragraph([
                    text("The field went on "),
                    text("past", ["italic"]),
                    text(" where the light gave out — "),
                    text("well", ["bold", "italic"]),
                    text(" past. 🜂"),
                ]),
                API.DocNode(
                    content: [paragraph([text("She counted fenceposts.")])],
                    type: "blockquote"
                ),
                API.DocNode(
                    content: [
                        API.DocNode(content: [paragraph([text("First")])], type: "listItem"),
                        API.DocNode(content: [paragraph([text("Second")])], type: "listItem"),
                    ],
                    type: "bulletList"
                ),
                API.DocNode(type: "horizontalRule"),
                paragraph([text("The house was still a long way off.")]),
            ],
            type: "doc"
        )
    }

    func testSwiftAgreesWithTheRealEditor() throws {
        let mine = document().blocks().map {
            Block(id: $0.id, type: $0.type, from: $0.from, to: $0.to)
        }
        XCTAssertEqual(mine, fromProseMirror)
    }

    /// The astral character is the reason this is worth checking: an emoji is
    /// one Swift `Character`, two UTF-16 code units, and ProseMirror counts
    /// the latter.
    func testTheAstralCharacterCostsTwo() throws {
        let blocks = document().blocks()
        let paragraph = blocks[1]
        XCTAssertEqual(paragraph.to - paragraph.from, 63)
        XCTAssertEqual(paragraph.text.count, 62)
    }
}
