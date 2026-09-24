import Foundation
import ScriptoriumKit

/// Owns the signed-in user and the single `APIClient` every view reaches for.
@MainActor
public final class SessionStore: ObservableObject {
    @Published public private(set) var user: API.User?
    @Published public private(set) var isLoading = true
    @Published public var errorMessage: String?

    public let client: APIClient
    /// Everything the views read and write goes through this, not the client:
    /// a read falls back to the phone, and a write is kept there first.
    public let sync: SyncEngine

    /// How many edits the server has not seen. Shown rather than hidden — a
    /// writer is owed the truth about whether their words have left the phone.
    @Published public private(set) var pending = 0
    @Published public private(set) var working = true

    public init(baseURL: URL, store: TokenStoring = KeychainTokenStore()) {
        let client = APIClient(baseURL: baseURL, tokens: TokenManager(store: store))
        self.client = client
        self.sync = SyncEngine(client: client, store: LocalStore(), queue: SaveQueue())
    }

    /// Called when the app comes back to the foreground, and after every save.
    public func catchUp() async {
        let outcomes = await sync.flush()
        working = !outcomes.values.contains(.queued)
        pending = await sync.pendingCount
    }

    public func refreshPendingCount() async {
        pending = await sync.pendingCount
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
        // A shared phone should not keep somebody else's unpublished novel.
        await sync.signOut()
        pending = 0
        user = nil
    }

    private func attempt(_ work: @escaping () async throws -> API.User) async {
        errorMessage = nil
        do {
            user = try await work()
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
