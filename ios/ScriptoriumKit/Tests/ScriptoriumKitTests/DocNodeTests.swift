import XCTest
@testable import ScriptoriumKit

/// These lock down the one piece of shared arithmetic that has to match the
/// Rust server and the web editor exactly: ProseMirror positions.
final class DocNodeTests: XCTestCase {
    private func doc(_ json: String) throws -> DocNode {
        try JSONDecoder().decode(DocNode.self, from: Data(json.utf8))
    }

    func testPlainTextFlattensBlocksOntoLines() throws {
        let node = try doc("""
        {"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"One."}]},
          {"type":"paragraph","content":[{"type":"text","text":"Two."}]}
        ]}
        """)
        XCTAssertEqual(node.plainText, "One.\nTwo.")
        XCTAssertEqual(node.wordCount, 2)
    }

    func testBlockPositionsMatchProseMirror() throws {
        let node = try doc("""
        {"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"Hello"}]},
          {"type":"paragraph","content":[{"type":"text","text":"World"}]}
        ]}
        """)
        let blocks = node.blocks()
        XCTAssertEqual(blocks.count, 2)
        // First paragraph: opens at 0, content runs 1..6.
        XCTAssertEqual(blocks[0].from, 1)
        XCTAssertEqual(blocks[0].to, 6)
        // Second paragraph starts after the first closes at 7.
        XCTAssertEqual(blocks[1].from, 8)
        XCTAssertEqual(blocks[1].to, 13)
    }

    func testLeafNodesOccupyOnePosition() throws {
        let node = try doc("""
        {"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"Hi"}]},
          {"type":"horizontalRule"},
          {"type":"paragraph","content":[{"type":"text","text":"Ho"}]}
        ]}
        """)
        let blocks = node.blocks()
        XCTAssertEqual(blocks[1].from, 4)
        XCTAssertEqual(blocks[1].to, 5)
        XCTAssertEqual(blocks[2].from, 6)
    }

    func testTextIsMeasuredInUTF16LikeTheWebEditor() throws {
        // An emoji is two UTF-16 code units, which is what ProseMirror counts.
        let node = try doc("""
        {"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"a🙂"}]}]}
        """)
        XCTAssertEqual(node.blocks()[0].to, 4)
    }

    func testRichFormattingIsDetected() throws {
        let plain = try doc("""
        {"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"plain"}]}]}
        """)
        let marked = try doc("""
        {"type":"doc","content":[{"type":"paragraph","content":[
          {"type":"text","text":"bold","marks":[{"type":"bold"}]}
        ]}]}
        """)
        XCTAssertFalse(plain.hasRichFormatting)
        XCTAssertTrue(marked.hasRichFormatting)
    }

    func testRoundTripThroughPlainTextPreservesParagraphs() {
        let node = DocNode.fromPlainText("First para.\n\nSecond para.")
        XCTAssertEqual(node.content?.count, 2)
        XCTAssertEqual(node.plainText, "First para.\nSecond para.")
    }
}
