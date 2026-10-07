import XCTest
@testable import JustTypeKit

/// A refused sign-in is a 401, the same status as an expired session. The
/// writer has to see what the server said, not "You are signed out".
final class SignInErrorTests: XCTestCase {
    private func client() -> APIClient {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [UnauthorizedStub.self]
        return APIClient(
            baseURL: URL(string: "https://example.test/api")!,
            tokens: TokenManager(store: MemoryTokenStore()),
            session: URLSession(configuration: configuration)
        )
    }

    func testWrongPasswordSaysSo() async {
        UnauthorizedStub.code = "unauthorized"
        UnauthorizedStub.message = "invalid email or password"
        do {
            _ = try await client().signIn(email: "a@example.com", password: "nope")
            XCTFail("sign-in should fail")
        } catch {
            XCTAssertEqual(
                error as? APIError,
                .http(status: 401, code: "unauthorized", message: "invalid email or password")
            )
        }
    }

    func testTwoFactorChallengeReachesTheCaller() async {
        UnauthorizedStub.code = "two_factor_required"
        UnauthorizedStub.message = "enter the code from your authenticator"
        do {
            _ = try await client().signIn(email: "a@example.com", password: "right")
            XCTFail("sign-in should ask for a code")
        } catch APIError.http(401, "two_factor_required", _) {
        } catch {
            XCTFail("unexpected \(error)")
        }
    }
}

private final class UnauthorizedStub: URLProtocol {
    nonisolated(unsafe) static var code = "unauthorized"
    nonisolated(unsafe) static var message = ""

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}

    override func startLoading() {
        let body = #"{"error":{"code":"\#(Self.code)","message":"\#(Self.message)"}}"#
        let response = HTTPURLResponse(
            url: request.url!, statusCode: 401, httpVersion: nil,
            headerFields: ["Content-Type": "application/json"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }
}
