import Foundation

/// A save the writer has made and the server has not seen.
public struct PendingSave: Codable, Equatable, Sendable {
    public let sceneId: String
    public var content: API.DocNode
    /// The hash the edit was made on top of. The server refuses a write whose
    /// base has moved, and a queued write is exactly the case where it might.
    public var baseHash: String?
    public var createRevision: Bool
    public var queuedAt: Date
    /// Set when the server refused it. The writer has to choose, so it stops
    /// being retried until they do.
    public var conflicted: Bool
    /// For a Daily Prompts answer: the prompt's day. Its `sceneId` is then
    /// `prompt:<day>`, the key the answer is queued under. Unset for a scene,
    /// and on saves queued before answers existed.
    public var day: String?
    /// For an answer: the writer's date when they wrote it, for their daily
    /// goal — not the date it finally goes up.
    public var localDay: String?

    public init(
        sceneId: String,
        content: API.DocNode,
        baseHash: String?,
        createRevision: Bool,
        queuedAt: Date = Date(),
        conflicted: Bool = false,
        day: String? = nil,
        localDay: String? = nil
    ) {
        self.sceneId = sceneId
        self.content = content
        self.baseHash = baseHash
        self.createRevision = createRevision
        self.queuedAt = queuedAt
        self.conflicted = conflicted
        self.day = day
        self.localDay = localDay
    }

    /// The key an answer to `day` is queued under.
    public static func entryKey(day: String) -> String { "prompt:\(day)" }
}

/// Saves waiting for a network, kept on disk.
///
/// One entry per scene, because a writer who typed for an hour in a tunnel
/// wants the paragraph as it stands, not a hundred versions of it on the way
/// there. The newest edit supersedes the one before it.
public actor SaveQueue {
    private let file: URL
    private var entries: [String: PendingSave] = [:]

    public init(directory: URL? = nil) {
        let base = directory ?? LocalStore.queueDirectory()
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        file = base.appendingPathComponent("pending.json")
        if let data = try? Data(contentsOf: file),
           let stored = try? JSONDecoder().decode([String: PendingSave].self, from: data) {
            entries = stored
        }
    }

    public func enqueue(_ save: PendingSave) {
        // The base hash of the *first* unsent edit is the one the server has
        // to be measured against; later edits in the same sitting were made on
        // top of text the server has never seen.
        var save = save
        if let existing = entries[save.sceneId], !existing.conflicted {
            save.baseHash = existing.baseHash
        }
        entries[save.sceneId] = save
        persist()
    }

    public func pending(for sceneId: String) -> PendingSave? { entries[sceneId] }

    public func all() -> [PendingSave] {
        entries.values.sorted { $0.queuedAt < $1.queuedAt }
    }

    public var count: Int { entries.count }

    public func remove(sceneId: String) {
        entries[sceneId] = nil
        persist()
    }

    public func markConflicted(sceneId: String) {
        entries[sceneId]?.conflicted = true
        persist()
    }

    /// The writer has chosen to keep their version: the base moves to whatever
    /// the server has now, and it goes back in the queue.
    public func rebase(sceneId: String, onto hash: String) {
        entries[sceneId]?.baseHash = hash
        entries[sceneId]?.conflicted = false
        persist()
    }

    public func clear() {
        entries = [:]
        persist()
    }

    private func persist() {
        guard let data = try? JSONEncoder().encode(entries) else { return }
        let temporary = file.appendingPathExtension("writing")
        do {
            try data.write(to: temporary, options: .atomic)
            _ = try FileManager.default.replaceItemAt(file, withItemAt: temporary)
        } catch {
            try? FileManager.default.removeItem(at: temporary)
        }
    }
}

public extension LocalStore {
    static func queueDirectory() -> URL {
        offlineDirectory()
    }
}
