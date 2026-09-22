import Foundation
import ScriptoriumKit

/// Owns the signed-in user and the single `APIClient` every view reaches for.
@MainActor
public final class SessionStore: ObservableObject {
    @Published public private(set) var user: User?
    @Published public private(set) var isLoading = true
    @Published public var errorMessage: String?

    public let client: APIClient

    public init(baseURL: URL, store: TokenStoring = KeychainTokenStore()) {
        self.client = APIClient(baseURL: baseURL, tokens: TokenManager(store: store))
    }

    /// A stored refresh token may be stale; asking the server is the only way
    /// to find out, and it costs one request at launch.
    public func restore() async {
        defer { isLoading = false }
        guard await client.tokenManager.isSignedIn else { return }
        user = try? await client.me()
    }

    public func signIn(email: String, password: String) async {
        await attempt { try await self.client.signIn(email: email, password: password).user }
    }

    public func register(email: String, password: String, displayName: String) async {
        await attempt {
            try await self.client.register(email: email, password: password, displayName: displayName).user
        }
    }

    public func signOut() async {
        await client.signOut()
        user = nil
    }

    private func attempt(_ work: @escaping () async throws -> User) async {
        errorMessage = nil
        do {
            user = try await work()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
