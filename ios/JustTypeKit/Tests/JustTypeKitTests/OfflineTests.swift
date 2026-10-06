import XCTest
@testable import JustTypeKit

/// Writing on a plane, which is 4.2's acceptance test.
///
/// The network is a stub that can be switched off mid-flight, so "no signal"
/// is a real condition here rather than a mocked method call: the client takes
/// the same URLSession path it always does and gets the same failure iOS would
/// hand it in a tunnel.
final class OfflineTests: XCTestCase {
    private var directory: URL!
    private var store: LocalStore!
    private var queue: SaveQueue!
    private var engine: SyncEngine!

    override func setUp() async throws {
        directory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent(UUID().uuidString)
        StubProtocol.reset()

        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [StubProtocol.self]
        let client = APIClient(
            baseURL: URL(string: "https://example.test/api")!,
            tokens: TokenManager(store: MemoryTokenStore()),
            session: URLSession(configuration: configuration)
        )
        store = LocalStore(directory: directory)
        queue = SaveQueue(directory: directory)
        engine = SyncEngine(client: client, store: store, queue: queue)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: directory)
    }

    // MARK: - The flight

    func testASaveWithNoNetworkIsKeptRatherThanLost() async throws {
        await store.putScene(Self.scene(text: "The field went on."))
        StubProtocol.online = false

        let outcome = await engine.save(
            sceneId: "scene-1",
            content: doc("The field went on past where the light gave out."),
            baseHash: "hash-1",
            createRevision: false
        )

        XCTAssertEqual(outcome, .queued)
        let pending = await engine.pending(for: "scene-1")
        XCTAssertEqual(pending?.content.plainText, "The field went on past where the light gave out.")
    }

    /// The bug this exists to prevent: reopening a scene offline must not show
    /// the server's older text over the writer's own unsent edit.
    func testReopeningOfflineShowsTheWritersOwnWords() async throws {
        await store.putScene(Self.scene(text: "The field went on."))
        StubProtocol.online = false
        _ = await engine.save(
            sceneId: "scene-1",
            content: doc("Her own newer sentence."),
            baseHash: "hash-1",
            createRevision: false
        )

        let reopened = await engine.scene(id: "scene-1")
        XCTAssertEqual(reopened?.scene.content.plainText, "Her own newer sentence.")
        XCTAssertEqual(reopened?.freshness, .local)
    }

    /// Several edits in one sitting collapse to one save, and it is measured
    /// against the hash the *first* one was made on top of — the later ones
    /// were written over text the server has never seen.
    func testASittingOfEditsBecomesOneSaveOnTheOriginalBase() async throws {
        await store.putScene(Self.scene(text: "The field went on."))
        StubProtocol.online = false
        for sentence in ["One.", "One. Two.", "One. Two. Three."] {
            _ = await engine.save(
                sceneId: "scene-1",
                content: doc(sentence),
                baseHash: "hash-1",
                createRevision: false
            )
        }

        let count = await engine.pendingCount
        XCTAssertEqual(count, 1)
        let pending = await engine.pending(for: "scene-1")
        XCTAssertEqual(pending?.content.plainText, "One. Two. Three.")
        XCTAssertEqual(pending?.baseHash, "hash-1")
    }

    // MARK: - Landing

    func testEverythingGoesWhenTheNetworkComesBack() async throws {
        await store.putScene(Self.scene(text: "The field went on."))
        StubProtocol.online = false
        _ = await engine.save(
            sceneId: "scene-1",
            content: doc("Written at 30,000 feet."),
            baseHash: "hash-1",
            createRevision: false
        )

        StubProtocol.online = true
        let outcomes = await engine.flush()

        XCTAssertEqual(outcomes["scene-1"], .saved(hash: "hash-2"))
        let remaining = await engine.pendingCount
        XCTAssertEqual(remaining, 0)
        XCTAssertEqual(StubProtocol.lastSavedText, "Written at 30,000 feet.")
    }

    /// The same conflict rule as 1.1: if the scene moved while the phone was
    /// offline, the queued edit is refused rather than applied over the top.
    func testAQueuedSaveOntoAMovedSceneConflicts() async throws {
        await store.putScene(Self.scene(text: "The field went on."))
        StubProtocol.online = false
        _ = await engine.save(
            sceneId: "scene-1",
            content: doc("Mine."),
            baseHash: "hash-1",
            createRevision: false
        )

        StubProtocol.online = true
        StubProtocol.conflict = true
        let outcomes = await engine.flush()

        guard case .conflict = outcomes["scene-1"] else {
            return XCTFail("expected a conflict, got \(String(describing: outcomes["scene-1"]))")
        }
        // Kept, and not retried: it is waiting on a person now.
        let pending = await engine.pending(for: "scene-1")
        XCTAssertEqual(pending?.conflicted, true)
        XCTAssertEqual(pending?.content.plainText, "Mine.")
    }

    func testKeepingMineRebasesOntoWhateverTheServerHasNow() async throws {
        await store.putScene(Self.scene(text: "The field went on."))
        StubProtocol.online = false
        _ = await engine.save(
            sceneId: "scene-1",
            content: doc("Mine."),
            baseHash: "hash-1",
            createRevision: false
        )
        StubProtocol.online = true
        StubProtocol.conflict = true
        _ = await engine.flush()

        StubProtocol.conflict = false
        let outcome = await engine.keepMine(sceneId: "scene-1")

        XCTAssertEqual(outcome, .saved(hash: "hash-2"))
        XCTAssertEqual(StubProtocol.lastSavedText, "Mine.")
        let remaining = await engine.pendingCount
        XCTAssertEqual(remaining, 0)
    }

    /// The queue is on disk, so a phone that dies in the air still has the
    /// words when it comes back on.
    func testTheQueueSurvivesTheAppBeingKilled() async throws {
        StubProtocol.online = false
        await store.putScene(Self.scene(text: "The field went on."))
        _ = await engine.save(
            sceneId: "scene-1",
            content: doc("Survives a crash."),
            baseHash: "hash-1",
            createRevision: false
        )

        // A second queue over the same directory is what the next launch sees.
        let reopened = SaveQueue(directory: directory)
        let pending = await reopened.pending(for: "scene-1")
        XCTAssertEqual(pending?.content.plainText, "Survives a crash.")
    }

    // MARK: - Helpers

    private func doc(_ text: String) -> API.DocNode { API.DocNode.fromPlainText(text) }

    private static func scene(text: String) -> API.SceneWithContent {
        API.SceneWithContent(
            chapterId: "chapter-1",
            contentHash: "hash-1",
            createdAt: "2026-01-01T00:00:00Z",
            id: "scene-1",
            position: 0,
            status: .drafting,
            title: "Opening Image",
            updatedAt: "2026-01-01T00:00:00Z",
            wordCount: 4,
            workId: "work-1",
            content: API.DocNode.fromPlainText(text)
        )
    }
}

/// A network that can be switched off.
private final class StubProtocol: URLProtocol {
    nonisolated(unsafe) static var online = true
    nonisolated(unsafe) static var conflict = false
    nonisolated(unsafe) static var lastSavedText: String?

    static func reset() {
        online = true
        conflict = false
        lastSavedText = nil
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }
    override func stopLoading() {}

    override func startLoading() {
        guard StubProtocol.online else {
            // The error iOS hands an app in a tunnel.
            client?.urlProtocol(
                self,
                didFailWithError: URLError(.notConnectedToInternet)
            )
            return
        }

        let path = request.url?.path ?? ""
        if path.hasSuffix("/content"), request.httpMethod == "PUT" {
            let body = request.httpBody ?? readBodyStream()
            if let body, let sent = try? JSONDecoder().decode(SaveBody.self, from: body) {
                StubProtocol.lastSavedText = sent.content.plainText
            }
            if StubProtocol.conflict {
                return respond(
                    status: 409,
                    json: #"{"error":{"code":"conflict","message":"this scene changed elsewhere"}}"#
                )
            }
            return respond(status: 200, json: Self.saveResult)
        }
        if path.contains("/scenes/") {
            return respond(status: 200, json: Self.sceneJSON)
        }
        respond(status: 404, json: #"{"error":{"code":"not_found","message":"no"}}"#)
    }

    /// `URLProtocol` gets a stream rather than `httpBody` for a body set by
    /// `URLSession`, which is the kind of thing that makes a stub silently
    /// assert nothing.
    private func readBodyStream() -> Data? {
        guard let stream = request.httpBodyStream else { return nil }
        stream.open()
        defer { stream.close() }
        var data = Data()
        let size = 4096
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: size)
        defer { buffer.deallocate() }
        while stream.hasBytesAvailable {
            let read = stream.read(buffer, maxLength: size)
            if read <= 0 { break }
            data.append(buffer, count: read)
        }
        return data
    }

    private func respond(status: Int, json: String) {
        let response = HTTPURLResponse(
            url: request.url!,
            statusCode: status,
            httpVersion: "HTTP/1.1",
            headerFields: ["Content-Type": "application/json"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(json.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }

    private struct SaveBody: Decodable {
        let content: API.DocNode
    }

    private static let sceneJSON = """
    {"id":"scene-1","workId":"work-1","chapterId":"chapter-1","title":"Opening Image",
     "position":0,"status":"drafting","wordCount":4,"contentHash":"hash-1",
     "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z",
     "content":{"type":"doc","content":[{"type":"paragraph","content":[
       {"type":"text","text":"The field went on."}]}]}}
    """

    private static let saveResult = """
    {"scene":{"id":"scene-1","workId":"work-1","chapterId":"chapter-1","title":"Opening Image",
     "position":0,"status":"drafting","wordCount":5,"contentHash":"hash-2",
     "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z"},
     "workWordCount":5}
    """
}

/// Tokens in memory: the Keychain is not the thing under test, and a
/// command-line test run has no entitlement to reach it anyway.
private final class MemoryTokenStore: TokenStoring, @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: String] = [:]

    func read(_ key: String) -> String? {
        lock.withLock { values[key] }
    }

    func write(_ value: String, for key: String) {
        lock.withLock { values[key] = value }
    }

    func delete(_ key: String) {
        lock.withLock { values[key] = nil }
    }
}
