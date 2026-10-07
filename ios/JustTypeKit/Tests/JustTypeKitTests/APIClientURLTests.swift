import XCTest
@testable import JustTypeKit

/// A query in a request path has to reach the server as a query: encoded into
/// the path, `me/writing?today=…` is a route that does not exist.
final class APIClientURLTests: XCTestCase {
    private let client = APIClient(
        baseURL: URL(string: "https://example.test/api")!,
        tokens: TokenManager(store: MemoryTokenStore())
    )

    func testPlainPath() {
        XCTAssertEqual(
            client.url(for: "works/abc/outline").absoluteString,
            "https://example.test/api/works/abc/outline"
        )
    }

    func testQueryStaysAQuery() {
        let url = client.url(for: "works/w1/versions/diff?from=a&to=b")
        XCTAssertEqual(url.absoluteString, "https://example.test/api/works/w1/versions/diff?from=a&to=b")
        XCTAssertEqual(url.path, "/api/works/w1/versions/diff")
        XCTAssertEqual(url.query, "from=a&to=b")
    }
}
