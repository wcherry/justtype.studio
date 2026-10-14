import XCTest
@testable import JustTypeKit
@testable import JustTypeUI

/// The export epic's fields as iOS shows them: read-only, but named the way
/// the web app and the exports name them.
final class BookDetailsTests: XCTestCase {
    func testFrontAndBackMatterAreNotNumbered() {
        let names = Naming.chapters([
            ("", "front"),
            ("", "chapter"),
            ("The Door", "chapter"),
            ("", "chapter"),
            ("Acknowledgments", "back"),
            ("", "back"),
        ])
        XCTAssertEqual(names, [
            "Front matter", "Chapter 1", "The Door", "Chapter 3", "Acknowledgments", "Back matter",
        ])
        XCTAssertEqual(Naming.roleBadge("front"), "Front matter")
        XCTAssertNil(Naming.roleBadge("chapter"))
    }

    func testPartsAreCountedOnTheirOwn() {
        let names = Naming.chapters([
            ("", "part"),
            ("", "chapter"),
            ("Interlude", "part"),
            ("", "chapter"),
            ("", "part"),
        ])
        XCTAssertEqual(names, ["Part 1", "Chapter 1", "Interlude", "Chapter 2", "Part 3"])
        XCTAssertEqual(Naming.roleBadge("part"), "Part")
        XCTAssertEqual(Naming.scene("", number: 1, role: "part"), "Part page")
        XCTAssertEqual(Naming.scene("", number: 1), "Scene 1")
    }

    func testMetadataDecodesFromTheServersShape() throws {
        let json = """
        {"language":"fr-CA","textDirection":null,"description":"Un roman.","publisher":"Petite Presse",
         "published":"2026-03","identifier":null,"keywords":["mer","famille"],
         "contributors":[{"name":"E. Ditor","role":"editor"}],
         "hasCover":true,"coverBytes":1234,"coverWidth":1600,"coverHeight":2560,
         "coverWarning":null,"updatedAt":"2026-10-09T00:00:00Z"}
        """
        let details = try JSONDecoder().decode(API.MetadataResponse.self, from: Data(json.utf8))
        XCTAssertEqual(details.language, "fr-CA")
        XCTAssertEqual(details.contributors.first?.role, "editor")
        XCTAssertEqual(details.keywords, ["mer", "famille"])
        XCTAssertTrue(details.hasCover)
        XCTAssertEqual(BookDetailsView.direction(nil), "As the language does")
        XCTAssertTrue(BookDetailsView.languageName("fr-CA").hasSuffix("· fr-CA"))
    }
}
