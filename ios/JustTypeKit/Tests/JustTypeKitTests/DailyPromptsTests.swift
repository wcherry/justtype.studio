import XCTest
@testable import JustTypeKit
@testable import JustTypeUI

/// The phone's side of Daily Prompts: what it decides for itself, which is
/// only how to lay things out and where a link goes. The rules are the server's.
final class DailyPromptsTests: XCTestCase {
    private func critique(_ id: String, parent: String?, author: String = "Ada") -> API.CritiqueView {
        API.CritiqueView(
            author: API.AuthorCard(displayName: author, id: author),
            body: "level \(id)", canRemove: false, createdAt: "t", editedAt: nil,
            entryId: "e", id: id, mine: false, parentId: parent, removed: false
        )
    }

    func testATwentyDeepThreadStaysWithinThreeIndents() {
        var list = [critique("0", parent: nil)]
        for i in 1..<20 { list.append(critique("\(i)", parent: "\(i - 1)", author: i % 2 == 0 ? "Ada" : "Bo")) }
        let rows = CritiquesView.arrange(list)
        XCTAssertEqual(rows.map(\.critique.id), (0..<20).map(String.init), "reading order is the thread's order")
        XCTAssertEqual(rows.map(\.depth).max(), 3)
        // Below the cap, each reply says whom it answers.
        XCTAssertNil(rows[3].replyingTo)
        XCTAssertEqual(rows[4].replyingTo, "Bo")
        XCTAssertEqual(rows[19].replyingTo, "Ada")
    }

    func testEmailAndInboxLinksOpenTheRightScreen() {
        XCTAssertEqual(PromptLink.route(path: "/prompts/2026-10-12/write"), .write("2026-10-12"))
        XCTAssertEqual(PromptLink.route(path: "/prompts/2026-10-12/entries/e1#critique-c1"), .entry("e1"))
        XCTAssertEqual(PromptLink.route(path: "/prompts/2026-10-12"), .day("2026-10-12"))
        XCTAssertNil(PromptLink.route(path: "/prompts/unsubscribe"))
        XCTAssertNil(PromptLink.route(path: "/works/w1"))
        XCTAssertTrue(PromptLink.handles(URL(string: "https://www.justtype.studio/prompts/2026-10-12/write")!))
    }

    func testTheClosingNoteMatchesTheWeb() {
        XCTAssertEqual(
            PromptPreferencesSection.closingNote(opensAt: "08:00", chosen: "05:00"),
            "Today's prompt closes at 08:00. You'll have 3 hours."
        )
        XCTAssertNil(PromptPreferencesSection.closingNote(opensAt: "08:00", chosen: "09:00"))
        XCTAssertNil(PromptPreferencesSection.closingNote(opensAt: "08:00", chosen: "08:00"))
    }

    func testASaveQueuedBeforeAnswersExistedStillReadsAsAScene() throws {
        let old = #"{"sceneId":"s1","content":{"type":"doc"},"baseHash":"h","createRevision":true,"queuedAt":0,"conflicted":false}"#
        let save = try JSONDecoder().decode(PendingSave.self, from: Data(old.utf8))
        XCTAssertNil(save.day)
        XCTAssertEqual(PendingSave.entryKey(day: "2026-10-12"), "prompt:2026-10-12")
    }

    func testAnAnswerReadsAsParagraphs() throws {
        let json = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"One."}]},{"type":"paragraph"},{"type":"paragraph","content":[{"type":"text","text":"Two"},{"type":"hardBreak"},{"type":"text","text":"lines."}]}]}"#
        let doc = try JSONDecoder().decode(API.DocNode.self, from: Data(json.utf8))
        XCTAssertEqual(doc.plainParagraphs, ["One.", "Two\nlines."])
    }
}
