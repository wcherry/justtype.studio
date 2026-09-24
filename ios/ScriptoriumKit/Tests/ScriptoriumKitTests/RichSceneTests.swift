import XCTest
@testable import ScriptoriumKit

/// The promise 4.1 makes: a scene written on the web opens on a phone and goes
/// back without losing a mark.
///
/// The fixture is not hand-written. It is a scene saved through the web
/// client's own API and read back from the server, so it carries exactly what
/// the server stores — including the attributes TipTap puts on a link, which
/// are the easiest thing in the document to drop.
final class RichSceneTests: XCTestCase {
    private func fixture() throws -> Data {
        let url = try XCTUnwrap(
            Bundle.module.url(forResource: "rich-scene", withExtension: "json", subdirectory: "Fixtures")
                ?? Bundle.module.url(forResource: "rich-scene", withExtension: "json")
        )
        return try Data(contentsOf: url)
    }

    /// Decode, re-encode, and compare against the server's own JSON. Anything
    /// the Swift model cannot hold shows up here as a missing key.
    func testNothingIsLostGoingBackToTheServer() throws {
        let original = try fixture()
        let document = try JSONDecoder().decode(API.DocNode.self, from: original)
        let returned = try JSONEncoder().encode(document)

        let before = try JSONSerialization.jsonObject(with: original)
        let after = try JSONSerialization.jsonObject(with: returned)
        XCTAssertTrue(
            NSDictionary(dictionary: before as! [String: Any])
                .isEqual(to: after as! [String: Any]),
            "the document changed on the way back:\n\(String(data: returned, encoding: .utf8) ?? "")"
        )
    }

    /// Named individually, so a failure says which mark went missing rather
    /// than that "something" did.
    func testEveryMarkSurvives() throws {
        let document = try JSONDecoder().decode(API.DocNode.self, from: fixture())
        var found: Set<String> = []
        func walk(_ node: API.DocNode) {
            for mark in node.marks ?? [] { found.insert(mark.type) }
            for child in node.content ?? [] { walk(child) }
        }
        walk(document)
        XCTAssertEqual(found, ["bold", "italic", "underline", "strike", "link"])
    }

    /// A link is a mark with attributes; losing the href would leave the words
    /// looking right and meaning nothing.
    func testALinkKeepsItsHref() throws {
        let document = try JSONDecoder().decode(API.DocNode.self, from: fixture())
        var href: String?
        func walk(_ node: API.DocNode) {
            for mark in node.marks ?? [] where mark.type == "link" {
                href = mark.attrs?.objectValue?["href"]?.stringValue
            }
            for child in node.content ?? [] { walk(child) }
        }
        walk(document)
        XCTAssertEqual(href, "https://example.com")
    }

    /// Structure as well as marks: the blocks the phone shows are the blocks
    /// the server has.
    func testTheBlockStructureIsTheServersOwn() throws {
        let document = try JSONDecoder().decode(API.DocNode.self, from: fixture())
        XCTAssertEqual(
            (document.content ?? []).map(\.type),
            ["heading", "paragraph", "blockquote", "bulletList", "horizontalRule"]
        )
    }
}
