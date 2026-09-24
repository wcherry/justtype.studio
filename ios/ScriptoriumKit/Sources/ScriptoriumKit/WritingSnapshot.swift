import Foundation

/// Today's writing, small enough to hand to something outside the app.
///
/// A widget cannot sign in, and it has no business holding a refresh token to
/// try. So the app writes what it knows to a file the widget can read, and the
/// widget's whole job is to draw it.
public struct WritingSnapshot: Codable, Equatable, Sendable {
    public var words: Int
    public var goal: Int?
    public var streak: Int
    /// What the writer would carry on with, for "start writing".
    public var sceneId: String?
    public var sceneTitle: String?
    public var workId: String?
    public var updatedAt: Date

    public init(
        words: Int = 0,
        goal: Int? = nil,
        streak: Int = 0,
        sceneId: String? = nil,
        sceneTitle: String? = nil,
        workId: String? = nil,
        updatedAt: Date = Date()
    ) {
        self.words = words
        self.goal = goal
        self.streak = streak
        self.sceneId = sceneId
        self.sceneTitle = sceneTitle
        self.workId = workId
        self.updatedAt = updatedAt
    }

    public var fraction: Double {
        guard let goal, goal > 0 else { return 0 }
        return min(1, Double(words) / Double(goal))
    }
}

/// Where the app leaves it and the widget finds it.
///
/// An App Group would be the tidy answer and needs a provisioning profile that
/// says so; this falls back to the app's own container when there is no group,
/// so the code path is the same either way and only the reach changes.
public enum SharedState {
    public static let appGroup = "group.app.scriptorium.ios"

    public static var containerURL: URL {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: appGroup)
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
    }

    private static var file: URL {
        containerURL.appendingPathComponent("writing-today.json")
    }

    public static func save(_ snapshot: WritingSnapshot) {
        guard let data = try? JSONEncoder().encode(snapshot) else { return }
        try? data.write(to: file, options: .atomic)
    }

    public static func load() -> WritingSnapshot? {
        guard let data = try? Data(contentsOf: file) else { return nil }
        return try? JSONDecoder().decode(WritingSnapshot.self, from: data)
    }
}

public extension ISO8601DateFormatter {
    /// The writer's own day, not UTC's. A writer past midnight is still
    /// working on yesterday as far as they are concerned, and the server takes
    /// the date the client hands it for exactly that reason.
    static let localDay: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withFullDate]
        formatter.timeZone = .current
        return formatter
    }()
}
