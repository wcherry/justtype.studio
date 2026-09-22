import Foundation

public enum APIError: LocalizedError, Equatable {
    /// The scene changed elsewhere since it was loaded. Reload before saving.
    case conflict(message: String)
    case http(status: Int, code: String, message: String)
    case transport(String)
    case decoding(String)
    case notAuthenticated

    public var errorDescription: String? {
        switch self {
        case .conflict(let message): message
        case .http(_, _, let message): message
        case .transport(let message): message
        case .decoding(let message): "Unexpected response: \(message)"
        case .notAuthenticated: "You are signed out."
        }
    }
}

private struct ErrorEnvelope: Decodable {
    struct Inner: Decodable {
        let code: String
        let message: String
    }
    let error: Inner
}

/// Holds the token pair and makes sure only one refresh is ever in flight.
public actor TokenManager {
    private enum Key {
        static let access = "accessToken"
        static let refresh = "refreshToken"
    }

    private let store: TokenStoring
    private var refreshTask: Task<Bool, Never>?

    public init(store: TokenStoring) {
        self.store = store
    }

    public var accessToken: String? { store.read(Key.access) }
    public var refreshToken: String? { store.read(Key.refresh) }
    public var isSignedIn: Bool { store.read(Key.refresh) != nil }

    public func save(_ session: Session) {
        store.write(session.accessToken, for: Key.access)
        store.write(session.refreshToken, for: Key.refresh)
    }

    public func clear() {
        store.delete(Key.access)
        store.delete(Key.refresh)
    }

    /// Coalesces concurrent refreshes: a burst of 401s produces one new pair.
    func refresh(using perform: @escaping @Sendable (String) async -> Session?) async -> Bool {
        if let refreshTask { return await refreshTask.value }
        guard let token = refreshToken else { return false }
        let task = Task<Bool, Never> { [weak self] in
            guard let session = await perform(token) else {
                await self?.clear()
                return false
            }
            await self?.save(session)
            return true
        }
        refreshTask = task
        let result = await task.value
        refreshTask = nil
        return result
    }
}

public struct APIClient: Sendable {
    public let baseURL: URL
    private let session: URLSession
    private let tokens: TokenManager

    public init(baseURL: URL, tokens: TokenManager, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.tokens = tokens
        self.session = session
    }

    public var tokenManager: TokenManager { tokens }

    // MARK: - Transport

    private func makeRequest(_ method: String, _ path: String, body: Data?) async -> URLRequest {
        var request = URLRequest(url: baseURL.appendingPathComponent(path))
        request.httpMethod = method
        if let body {
            request.httpBody = body
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let token = await tokens.accessToken {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        return request
    }

    private func send<T: Decodable>(
        _ method: String,
        _ path: String,
        body: Encodable? = nil,
        retryOn401: Bool = true
    ) async throws -> T {
        let payload = try body.map { try JSONEncoder().encode(AnyEncodable($0)) }
        let request = await makeRequest(method, path, body: payload)

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw APIError.transport(error.localizedDescription)
        }

        guard let http = response as? HTTPURLResponse else {
            throw APIError.transport("no HTTP response")
        }

        if http.statusCode == 401, retryOn401 {
            let refreshed = await tokens.refresh { [self] token in
                try? await refreshSession(refreshToken: token)
            }
            if refreshed {
                return try await send(method, path, body: body, retryOn401: false)
            }
            throw APIError.notAuthenticated
        }

        guard (200..<300).contains(http.statusCode) else {
            if http.statusCode == 409,
               let envelope = try? JSONDecoder().decode(ErrorEnvelope.self, from: data) {
                throw APIError.conflict(message: envelope.error.message)
            }
            if let envelope = try? JSONDecoder().decode(ErrorEnvelope.self, from: data) {
                throw APIError.http(
                    status: http.statusCode,
                    code: envelope.error.code,
                    message: envelope.error.message
                )
            }
            throw APIError.http(status: http.statusCode, code: "unknown", message: "Request failed")
        }

        if T.self == EmptyResponse.self, let empty = EmptyResponse() as? T {
            return empty
        }
        do {
            return try JSONDecoder().decode(T.self, from: data)
        } catch {
            throw APIError.decoding(String(describing: error))
        }
    }

    public struct EmptyResponse: Decodable, Sendable {
        public init() {}
    }

    // MARK: - Auth

    private func refreshSession(refreshToken: String) async throws -> Session {
        var request = URLRequest(url: baseURL.appendingPathComponent("auth/refresh"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(["refreshToken": refreshToken])
        let (data, response) = try await session.data(for: request)
        guard let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw APIError.notAuthenticated
        }
        return try JSONDecoder().decode(Session.self, from: data)
    }

    public func register(email: String, password: String, displayName: String) async throws -> Session {
        let session: Session = try await send(
            "POST", "auth/register",
            body: ["email": email, "password": password, "displayName": displayName]
        )
        await tokens.save(session)
        return session
    }

    public func signIn(email: String, password: String) async throws -> Session {
        let session: Session = try await send(
            "POST", "auth/login",
            body: ["email": email, "password": password]
        )
        await tokens.save(session)
        return session
    }

    public func signOut() async {
        if let refreshToken = await tokens.refreshToken {
            _ = try? await send("POST", "auth/logout", body: ["refreshToken": refreshToken]) as EmptyResponse
        }
        await tokens.clear()
    }

    public func me() async throws -> User {
        try await send("GET", "auth/me")
    }

    // MARK: - Writing

    public func templates() async throws -> [Template] {
        try await send("GET", "templates")
    }

    public func works() async throws -> [Work] {
        try await send("GET", "works")
    }

    public func createWork(title: String, kind: WorkKind, templateId: String?) async throws -> Outline {
        struct Body: Encodable {
            let title: String
            let kind: String
            let templateId: String?
        }
        return try await send("POST", "works", body: Body(title: title, kind: kind.rawValue, templateId: templateId))
    }

    public func outline(workId: String) async throws -> Outline {
        try await send("GET", "works/\(workId)/outline")
    }

    public func scene(id: String) async throws -> SceneWithContent {
        try await send("GET", "scenes/\(id)")
    }

    public func createScene(chapterId: String, title: String) async throws -> SceneSummary {
        try await send("POST", "chapters/\(chapterId)/scenes", body: ["title": title])
    }

    public func createChapter(workId: String, title: String) async throws -> Chapter {
        try await send("POST", "works/\(workId)/chapters", body: ["title": title])
    }

    /// `baseHash` is the `contentHash` this edit was made on top of. A mismatch
    /// comes back as `APIError.conflict`, which the caller resolves by reloading
    /// and re-sending — never by retrying blindly.
    public func saveScene(
        id: String,
        content: DocNode,
        createRevision: Bool = true,
        baseHash: String? = nil
    ) async throws -> SaveResult {
        struct Body: Encodable {
            let content: DocNode
            let createRevision: Bool
            let baseHash: String?
        }
        return try await send(
            "PUT", "scenes/\(id)/content",
            body: Body(content: content, createRevision: createRevision, baseHash: baseHash)
        )
    }

    public func restoreRevision(sceneId: String, revisionId: String) async throws -> SaveResult {
        try await send("POST", "scenes/\(sceneId)/revisions/\(revisionId)/restore", body: [String: String]())
    }

    public func updateScene(id: String, title: String? = nil, status: String? = nil) async throws -> SceneSummary {
        struct Body: Encodable {
            let title: String?
            let status: String?
        }
        return try await send("PATCH", "scenes/\(id)", body: Body(title: title, status: status))
    }

    // MARK: - Versions

    public func versions(workId: String) async throws -> [Version] {
        try await send("GET", "works/\(workId)/versions")
    }

    public func snapshot(workId: String, label: String?) async throws -> VersionDetail {
        struct Body: Encodable { let label: String? }
        return try await send("POST", "works/\(workId)/versions", body: Body(label: label))
    }

    public func publish(versionId: String) async throws -> Version {
        try await send("POST", "versions/\(versionId)/publish", body: [String: String]())
    }

    public func finalize(versionId: String) async throws -> Version {
        try await send("POST", "versions/\(versionId)/finalize", body: [String: String]())
    }

    public func diff(workId: String, from: String, to: String) async throws -> DiffResponse {
        try await send("GET", "works/\(workId)/versions/diff?from=\(from)&to=\(to)")
    }

    // MARK: - Publishing and review

    public func offerForReview(
        versionId: String,
        visibility: ReviewVisibility,
        reviewPrompt: String?
    ) async throws -> PublicationCard {
        struct Body: Encodable {
            let visibility: String
            let scope: String
            let reviewPrompt: String?
        }
        return try await send(
            "POST", "versions/\(versionId)/publications",
            body: Body(visibility: visibility.rawValue, scope: "work", reviewPrompt: reviewPrompt)
        )
    }

    public func feed(mine: Bool = false) async throws -> [PublicationCard] {
        try await send("GET", "publications\(mine ? "?mine=true" : "")")
    }

    public func publication(id: String) async throws -> PublicationCard {
        try await send("GET", "publications/\(id)")
    }

    public func publicationContent(id: String) async throws -> VersionContent {
        try await send("GET", "publications/\(id)/content")
    }

    // MARK: - Comments

    public func comments(publicationId: String, sceneId: String? = nil) async throws -> [CommentView] {
        var path = "publications/\(publicationId)/comments"
        if let sceneId { path += "?sceneId=\(sceneId)" }
        return try await send("GET", path)
    }

    public func addComment(
        publicationId: String,
        body: String,
        sceneId: String? = nil,
        parentId: String? = nil,
        anchor: (from: Int, to: Int)? = nil,
        anchorQuote: String? = nil,
        category: CommentCategory = .general
    ) async throws -> CommentView {
        struct Anchor: Encodable {
            let from: Int
            let to: Int
        }
        struct Body: Encodable {
            let body: String
            let sceneId: String?
            let parentId: String?
            let anchor: Anchor?
            let anchorQuote: String?
            let category: String
        }
        return try await send(
            "POST", "publications/\(publicationId)/comments",
            body: Body(
                body: body,
                sceneId: sceneId,
                parentId: parentId,
                anchor: anchor.map { Anchor(from: $0.from, to: $0.to) },
                anchorQuote: anchorQuote,
                category: category.rawValue
            )
        )
    }

    public func vote(commentId: String, value: Int) async throws -> VoteResult {
        try await send("POST", "comments/\(commentId)/vote", body: ["value": value])
    }

    public func resolve(commentId: String) async throws -> Comment {
        try await send("PATCH", "comments/\(commentId)", body: ["state": "resolved"])
    }
}

/// Lets `send` take any `Encodable` without generic plumbing at every call site.
private struct AnyEncodable: Encodable {
    private let encode: (Encoder) throws -> Void

    init(_ wrapped: Encodable) {
        encode = wrapped.encode
    }

    func encode(to encoder: Encoder) throws {
        try encode(encoder)
    }
}
