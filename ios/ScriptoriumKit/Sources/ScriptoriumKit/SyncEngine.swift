import Foundation

/// What became of a save.
public enum SaveOutcome: Equatable, Sendable {
    /// The server has it. Carries the new content hash to build the next edit on.
    case saved(hash: String)
    /// No network. It is on disk and will go when there is one.
    case queued
    /// The server refused it: the scene moved under the edit.
    case conflict(message: String)
}

/// What a read came from, so the writer can be told.
public enum Freshness: Equatable, Sendable {
    case server
    /// From the phone. Either there is no network, or there are edits the
    /// server has not seen yet — and those win over anything it would send.
    case local
}

/// The layer between the views and the network.
///
/// Every read prefers the server and falls back to the phone. Every write goes
/// to the phone first and to the server if it can. The rule that matters most
/// is the one in `scene(id:)`: a queued edit beats the server's copy, always.
/// Handing a writer the server's older text because a save had not gone
/// through yet would silently undo their work in front of them.
public actor SyncEngine {
    private let client: APIClient
    private let store: LocalStore
    private let queue: SaveQueue

    public init(client: APIClient, store: LocalStore, queue: SaveQueue) {
        self.client = client
        self.store = store
        self.queue = queue
    }

    // MARK: - Reading

    public func works() async -> (works: [API.Work], freshness: Freshness)? {
        if let fresh = try? await client.works() {
            await store.putWorks(fresh)
            return (fresh, .server)
        }
        guard let cached = await store.works() else { return nil }
        return (cached, .local)
    }

    public func outline(workId: String) async -> (outline: API.OutlineResponse, freshness: Freshness)? {
        if let fresh = try? await client.outline(workId: workId) {
            await store.putOutline(fresh)
            return (fresh, .server)
        }
        guard let cached = await store.outline(workId: workId) else { return nil }
        return (cached, .local)
    }

    public func scene(id: String) async -> (scene: API.SceneWithContent, freshness: Freshness)? {
        let fresh = try? await client.scene(id: id)
        if let fresh { await store.putScene(fresh) }

        // A pending edit is newer than anything the server can offer, and the
        // hash it was made on top of is the one the next save must carry.
        if let pending = await queue.pending(for: id) {
            // `??` takes an autoclosure, which cannot await; spelled out.
            var base = fresh
            if base == nil { base = await store.scene(id: id) }
            guard var scene = base else { return nil }
            scene.content = pending.content
            scene.contentHash = pending.baseHash ?? scene.contentHash
            return (scene, .local)
        }

        if let fresh { return (fresh, .server) }
        guard let cached = await store.scene(id: id) else { return nil }
        return (cached, .local)
    }

    // MARK: - Writing

    public func save(
        sceneId: String,
        content: API.DocNode,
        baseHash: String?,
        createRevision: Bool
    ) async -> SaveOutcome {
        // Disk first. Whatever happens to the network, the words are kept.
        if var scene = await store.scene(id: sceneId) {
            scene.content = content
            await store.putScene(scene)
        }

        do {
            let result = try await client.saveScene(
                id: sceneId,
                content: content,
                createRevision: createRevision,
                baseHash: baseHash
            )
            await queue.remove(sceneId: sceneId)
            // The scene the server just confirmed, with the content it now
            // holds — built from the row it sent rather than from what was
            // cached, so a rename on another device does not come back.
            if var cached = await store.scene(id: sceneId) {
                cached.content = content
                cached.contentHash = result.scene.contentHash
                cached.wordCount = result.scene.wordCount
                cached.updatedAt = result.scene.updatedAt
                await store.putScene(cached)
            }
            return .saved(hash: result.scene.contentHash)
        } catch APIError.conflict(let message) {
            await queue.enqueue(
                PendingSave(
                    sceneId: sceneId,
                    content: content,
                    baseHash: baseHash,
                    createRevision: createRevision,
                    conflicted: true
                )
            )
            return .conflict(message: message)
        } catch {
            // Anything else — no network, a dead server, a tunnel — is a
            // reason to wait, not a reason to lose the paragraph.
            await queue.enqueue(
                PendingSave(
                    sceneId: sceneId,
                    content: content,
                    baseHash: baseHash,
                    createRevision: createRevision
                )
            )
            return .queued
        }
    }

    // MARK: - Catching up

    public var pendingCount: Int {
        get async { await queue.count }
    }

    public func pending(for sceneId: String) async -> PendingSave? {
        await queue.pending(for: sceneId)
    }

    /// Send everything waiting. Called when the app comes back to life.
    ///
    /// A conflicted entry is left alone: it is waiting on a person, and
    /// retrying it can only fail the same way.
    @discardableResult
    public func flush() async -> [String: SaveOutcome] {
        var outcomes: [String: SaveOutcome] = [:]
        for entry in await queue.all() where !entry.conflicted {
            let outcome = await save(
                sceneId: entry.sceneId,
                content: entry.content,
                baseHash: entry.baseHash,
                createRevision: entry.createRevision
            )
            outcomes[entry.sceneId] = outcome
            // Still no network: the rest will fare no better, and hammering a
            // dead connection is how a phone gets warm on a plane.
            if outcome == .queued { break }
        }
        return outcomes
    }

    /// The writer chose their version over the server's.
    public func keepMine(sceneId: String) async -> SaveOutcome {
        guard let pending = await queue.pending(for: sceneId) else { return .queued }
        guard let latest = try? await client.scene(id: sceneId) else { return .queued }
        await queue.rebase(sceneId: sceneId, onto: latest.contentHash)
        return await save(
            sceneId: sceneId,
            content: pending.content,
            baseHash: latest.contentHash,
            createRevision: true
        )
    }

    /// The writer chose the server's version. Their queued edit goes.
    public func discardMine(sceneId: String) async {
        await queue.remove(sceneId: sceneId)
    }

    public func signOut() async {
        await queue.clear()
        await store.clear()
    }
}
