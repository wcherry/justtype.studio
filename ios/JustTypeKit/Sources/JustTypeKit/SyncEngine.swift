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

/// What became of saving a Daily Prompts answer.
public enum EntrySaveOutcome: Equatable, Sendable {
    case saved(API.PromptEntryView)
    case queued
    /// Saved elsewhere since: the writer chooses which to keep.
    case conflict(message: String)
    /// Refused for good (not enrolled, say). Not queued: it would fail again.
    case refused(message: String)
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

    // MARK: - Daily Prompts answers

    /// The writer's answer to `day`, and the document to open: a queued edit
    /// wins over the server's copy, as it does for a scene.
    public func entry(day: String) async -> (entry: API.PromptEntryView?, document: API.DocNode?, freshness: Freshness) {
        let key = PendingSave.entryKey(day: day)
        // Reached the server: its answer, or none started. Did not: the copy here.
        let reached: Bool
        var entry: API.PromptEntryView?
        do {
            entry = try await client.entry(day: day)
            reached = true
            if let entry { await store.putEntry(entry) }
        } catch {
            entry = await store.entry(day: day)
            reached = false
        }
        if let pending = await queue.pending(for: key) {
            return (entry, pending.content, .local)
        }
        return (entry, entry?.document, reached ? .server : .local)
    }

    public func saveEntry(day: String, content: API.DocNode, baseHash: String?, localDay: String) async -> EntrySaveOutcome {
        let key = PendingSave.entryKey(day: day)
        do {
            let saved = try await client.saveEntry(day: day, content: content, baseHash: baseHash, localDay: localDay)
            await queue.remove(sceneId: key)
            await store.putEntry(saved)
            return .saved(saved)
        } catch APIError.conflict(let message) {
            await queue.enqueue(PendingSave(sceneId: key, content: content, baseHash: baseHash, createRevision: false, conflicted: true, day: day, localDay: localDay))
            return .conflict(message: message)
        } catch APIError.http(let status, _, let message) where (400..<500).contains(status) && status != 408 && status != 429 {
            // Refused for good (not enrolled, say): queueing would retry forever.
            return .refused(message: message)
        } catch {
            await queue.enqueue(PendingSave(sceneId: key, content: content, baseHash: baseHash, createRevision: false, day: day, localDay: localDay))
            return .queued
        }
    }

    /// The writer keeps their answer over the one saved elsewhere.
    public func keepMyEntry(day: String) async -> EntrySaveOutcome {
        let key = PendingSave.entryKey(day: day)
        guard let pending = await queue.pending(for: key) else { return .queued }
        guard let latest = try? await client.entry(day: day) else { return .queued }
        await queue.rebase(sceneId: key, onto: latest.contentHash)
        return await saveEntry(day: day, content: pending.content, baseHash: latest.contentHash, localDay: pending.localDay ?? Self.today())
    }

    public func discardMyEntry(day: String) async {
        await queue.remove(sceneId: PendingSave.entryKey(day: day))
    }

    static func today() -> String {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: Date())
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
            if let day = entry.day {
                let sent = await saveEntry(day: day, content: entry.content, baseHash: entry.baseHash, localDay: entry.localDay ?? Self.today())
                let outcome: SaveOutcome
                switch sent {
                case .saved(let saved): outcome = .saved(hash: saved.contentHash)
                case .queued: outcome = .queued
                case .conflict(let message), .refused(let message): outcome = .conflict(message: message)
                }
                outcomes[entry.sceneId] = outcome
                if outcome == .queued { break }
                continue
            }
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
