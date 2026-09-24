import XCTest
@testable import ScriptoriumKit

/// **Spike — Phase 4.1, path A.** What survives a round trip through an
/// `NSAttributedString`, and what does not.
///
/// The tests are the finding. Each one names a thing a writer actually did and
/// asserts whether it comes back, so the answer is a list rather than an
/// impression.
final class AttributedDocTests: XCTestCase {
    /// A scene with the formatting a novelist actually uses.
    private func sample() -> API.DocNode {
        API.DocNode(
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
                        API.DocNode(text: " where the light gave out — ", type: "text"),
                        API.DocNode(
                            marks: [API.DocMark(type: "bold"), API.DocMark(type: "italic")],
                            text: "well",
                            type: "text"
                        ),
                        API.DocNode(text: " past. 🜂", type: "text"),
                    ],
                    type: "paragraph"
                ),
                API.DocNode(
                    content: [
                        API.DocNode(
                            content: [API.DocNode(text: "She counted fenceposts.", type: "text")],
                            type: "paragraph"
                        )
                    ],
                    type: "blockquote"
                ),
                API.DocNode(
                    content: [
                        API.DocNode(
                            content: [
                                API.DocNode(
                                    content: [API.DocNode(text: "First", type: "text")],
                                    type: "paragraph"
                                )
                            ],
                            type: "listItem"
                        ),
                        API.DocNode(
                            content: [
                                API.DocNode(
                                    content: [API.DocNode(text: "Second", type: "text")],
                                    type: "paragraph"
                                )
                            ],
                            type: "listItem"
                        ),
                    ],
                    type: "bulletList"
                ),
                API.DocNode(type: "horizontalRule"),
                API.DocNode(
                    content: [API.DocNode(text: "The house was still a long way off.", type: "text")],
                    type: "paragraph"
                ),
            ],
            type: "doc"
        )
    }

    private func roundTrip(_ doc: API.DocNode) -> API.DocNode {
        AttributedDoc.document(from: AttributedDoc.attributedString(from: doc))
    }

    func testBlockStructureSurvives() throws {
        let after = roundTrip(sample())
        XCTAssertEqual(
            (after.content ?? []).map(\.type),
            ["heading", "paragraph", "blockquote", "bulletList", "horizontalRule", "paragraph"]
        )
    }

    func testHeadingKeepsItsLevel() throws {
        let after = roundTrip(sample())
        let heading = try XCTUnwrap(after.content?.first)
        XCTAssertEqual(heading.attrs?.objectValue?["level"]?.intValue, 2)
    }

    func testMarksSurviveIncludingCombinations() throws {
        let after = roundTrip(sample())
        let paragraph = try XCTUnwrap(after.content?[1])
        let marked = (paragraph.content ?? []).compactMap { node -> (String, [String])? in
            guard let marks = node.marks, !marks.isEmpty else { return nil }
            return (node.text ?? "", marks.map(\.type).sorted())
        }
        XCTAssertEqual(marked.count, 2)
        XCTAssertEqual(marked[0].0, "past")
        XCTAssertEqual(marked[0].1, ["italic"])
        XCTAssertEqual(marked[1].0, "well")
        XCTAssertEqual(marked[1].1, ["bold", "italic"])
    }

    func testTextIsIdenticalIncludingAstralCharacters() throws {
        let before = sample()
        XCTAssertEqual(roundTrip(before).plainText, before.plainText)
    }

    func testListItemsStayInOneList() throws {
        let after = roundTrip(sample())
        let list = try XCTUnwrap(after.content?[3])
        XCTAssertEqual(list.type, "bulletList")
        XCTAssertEqual(list.content?.count, 2)
    }

    func testALinkKeepsItsHref() throws {
        let doc = API.DocNode(
            content: [
                API.DocNode(
                    content: [
                        API.DocNode(
                            marks: [
                                API.DocMark(
                                    attrs: .object(["href": .string("https://example.com")]),
                                    type: "link"
                                )
                            ],
                            text: "example",
                            type: "text"
                        )
                    ],
                    type: "paragraph"
                )
            ],
            type: "doc"
        )
        let text = try XCTUnwrap(roundTrip(doc).content?.first?.content?.first)
        XCTAssertEqual(text.marks?.first?.type, "link")
        XCTAssertEqual(
            text.marks?.first?.attrs?.objectValue?["href"]?.stringValue,
            "https://example.com"
        )
    }

    // MARK: - The losses

    /// A nested list flattens: one level of depth is recorded, but the
    /// converter rebuilds every item at the top level.
    func testNestedListsFlatten() throws {
        let inner = API.DocNode(
            content: [
                API.DocNode(
                    content: [
                        API.DocNode(
                            content: [API.DocNode(text: "Inner", type: "text")],
                            type: "paragraph"
                        )
                    ],
                    type: "listItem"
                )
            ],
            type: "bulletList"
        )
        let doc = API.DocNode(
            content: [
                API.DocNode(
                    content: [
                        API.DocNode(
                            content: [
                                API.DocNode(
                                    content: [API.DocNode(text: "Outer", type: "text")],
                                    type: "paragraph"
                                ),
                                inner,
                            ],
                            type: "listItem"
                        )
                    ],
                    type: "bulletList"
                )
            ],
            type: "doc"
        )
        let after = roundTrip(doc)
        let list = try XCTUnwrap(after.content?.first)
        // Both items come back, at the same level: the nesting is gone.
        XCTAssertEqual(list.content?.count, 2)
    }

    /// Two paragraphs inside one blockquote come back as two — but a
    /// blockquote immediately followed by another is merged into one, because
    /// a flat string has nowhere to record where one ended.
    func testAdjacentBlockquotesMerge() throws {
        let quote = { (text: String) in
            API.DocNode(
                content: [
                    API.DocNode(
                        content: [API.DocNode(text: text, type: "text")],
                        type: "paragraph"
                    )
                ],
                type: "blockquote"
            )
        }
        let doc = API.DocNode(content: [quote("First."), quote("Second.")], type: "doc")
        let after = roundTrip(doc)
        XCTAssertEqual(after.content?.count, 1)
        XCTAssertEqual(after.content?.first?.content?.count, 2)
    }

    /// The position contract, which is the part that would break comments.
    ///
    /// ProseMirror counts a block boundary as two positions (one to close, one
    /// to open); an attributed string counts one character. So an offset in
    /// the editor is not a ProseMirror position, and every anchor has to be
    /// mapped — this test measures the drift rather than asserting it away.
    func testOffsetsAreNotProseMirrorPositions() throws {
        let doc = sample()
        let attributed = AttributedDoc.attributedString(from: doc)
        let blocks = doc.blocks()

        // The second block's text starts here in ProseMirror coordinates…
        let pmStart = blocks[1].from
        // …and here in the attributed string.
        let stringStart = (attributed.string as NSString)
            .range(of: "The field went on").location

        XCTAssertNotEqual(pmStart, stringStart)
        // After one heading: two positions of drift — ProseMirror spends one
        // closing the heading and one opening the paragraph, where the string
        // spends a single newline.
        XCTAssertEqual(pmStart - stringStart, 2)

        // By the last paragraph it is thirteen, not six, because the drift
        // follows *nesting* and not block count: the blockquote and the list
        // each wrap their contents in another node, and every wrapper costs
        // two more positions the string never spends. So a mapping cannot be
        // an offset — it has to walk the document.
        let laterPM = blocks[5].from
        let laterString = (attributed.string as NSString).range(of: "The house").location
        XCTAssertEqual(laterPM - laterString, 13)
    }
}
