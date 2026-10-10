import Foundation
import JustTypeKit
#if canImport(WidgetKit)
    import WidgetKit
#endif

/// Owns the signed-in user and the single `APIClient` every view reaches for.
@MainActor
public final class SessionStore: ObservableObject {
    @Published public private(set) var user: API.User?
    @Published public private(set) var isLoading = true
    @Published public var errorMessage: String?
    /// The password was right; the account wants a code as well.
    @Published public var needsCode = false

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

    /// Unread in the inbox, for the tab's badge. Read on coming to the front
    /// and on opening the inbox: there is no push yet, so a badge on the app
    /// icon would be stale most of the time and is not shown.
    @Published public private(set) var unread = 0

    public func refreshUnread() async {
        if let count = try? await client.unreadCount() { unread = count }
    }

    public func setUnread(_ count: Int) { unread = count }

    /// After a setting changes, so every screen reads the account's value.
    public func replaceUser(_ next: API.User) { user = next }

    /// Called when the app comes back to the foreground, and after every save.
    public func catchUp() async {
        await refreshUnread()
        let outcomes = await sync.flush()
        working = !outcomes.values.contains(.queued)
        pending = await sync.pendingCount
        await publishWritingSnapshot()
    }

    public func refreshPendingCount() async {
        pending = await sync.pendingCount
    }

    /// Leaves today's writing where the widget and the Shortcut can find it.
    ///
    /// Written by the app because the app is the only thing here that can
    /// sign in; an extension holding a refresh token to fetch its own copy
    /// would be a credential in a second place for a progress bar.
    public func publishWritingSnapshot(lastScene: (id: String, title: String, workId: String)? = nil) async {
        let today = ISO8601DateFormatter.localDay.string(from: Date())
        guard let summary = try? await client.writing(today: today) else { return }
        SharedState.save(
            WritingSnapshot(
                words: summary.wordsToday,
                goal: summary.dailyWordGoal,
                streak: summary.streak,
                weekWords: summary.wordsThisWeek,
                weekGoal: summary.weeklyWordGoal,
                sceneId: lastScene?.id ?? SharedState.load()?.sceneId,
                sceneTitle: lastScene?.title ?? SharedState.load()?.sceneTitle,
                workId: lastScene?.workId ?? SharedState.load()?.workId
            )
        )
        #if canImport(WidgetKit)
            WidgetCenter.shared.reloadAllTimelines()
        #endif
    }

    /// A stored refresh token may be stale; asking the server is the only way
    /// to find out, and it costs one request at launch.
    public func restore() async {
        defer { isLoading = false }
        guard await client.tokenManager.isSignedIn else { return }
        user = try? await client.me()
    }

    public func signIn(email: String, password: String, code: String? = nil) async {
        await attempt {
            try await self.client.signIn(email: email, password: password, code: code).user
        }
    }

    public func register(email: String, password: String, displayName: String, acceptTerms: Bool) async {
        await attempt {
            try await self.client.register(
                email: email, password: password, displayName: displayName, acceptTerms: acceptTerms
            ).user
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
            needsCode = false
        } catch APIError.http(_, "two_factor_required", _) {
            // Not a failure: ask for the code and go again.
            needsCode = true
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}
