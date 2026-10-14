import XCTest
@testable import JustTypeKit
@testable import JustTypeUI

/// The summary email setting: the phone only chooses how often and says
/// where it is; when it goes and what is in it are the server's.
final class SummaryEmailTests: XCTestCase {
    func testTheServersAnswerReads() throws {
        let off = #"{"often":"none","timezone":null,"sendTime":"07:00","emailVerified":true}"#
        let view = try JSONDecoder().decode(API.SummaryEmailView.self, from: Data(off.utf8))
        XCTAssertEqual(view.often, .none)
        XCTAssertNil(view.timezone)
        XCTAssertEqual(view.sendTime, "07:00")

        let on = #"{"often":"weekly","timezone":"Europe/London","sendTime":"07:00","emailVerified":true}"#
        XCTAssertEqual(try JSONDecoder().decode(API.SummaryEmailView.self, from: Data(on.utf8)).often, .weekly)
    }

    func testAChoiceGoesWithTheDevicesTimezone() throws {
        let body = API.UpdateSummaryEmailRequest(often: .daily, timezone: "Asia/Tokyo")
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(body)) as? [String: String]
        XCTAssertEqual(json, ["often": "daily", "timezone": "Asia/Tokyo"])
    }

    func testEveryChoiceHasWords() {
        XCTAssertEqual(API.SummaryEmail.allCases.map(\.label), ["None", "Daily", "Weekly"])
        XCTAssertTrue(API.SummaryEmail.allCases.allSatisfy { !$0.hint.isEmpty })
    }
}
