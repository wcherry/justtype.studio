import Foundation

public struct User: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let email: String
    public let displayName: String
    public let bio: String?
    public let createdAt: String
    public let updatedAt: String
}

public struct Session: Codable, Equatable, Sendable {
    public let user: User
    public let accessToken: String
    public let refreshToken: String
    public let expiresIn: Int
}

public struct Template: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let kind: String
    public let description: String
    public let builtIn: Bool
}

public enum WorkKind: String, Codable, CaseIterable, Hashable, Sendable {
    case novel, novella
    case shortStory = "short_story"
    case serial

    public var label: String {
        switch self {
        case .novel: "Novel"
        case .novella: "Novella"
        case .shortStory: "Short story"
        case .serial: "Serial"
        }
    }
}

public struct Work: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let ownerId: String
    public let title: String
    public let subtitle: String?
    public let kind: WorkKind
    public let synopsis: String?
    public let templateId: String?
    public let status: String
    public let currentVersionId: String?
    public let wordCount: Int
    public let createdAt: String
    public let updatedAt: String
}

public struct Chapter: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let workId: String
    public let title: String
    public let summary: String?
    public let position: Int
}

public struct SceneSummary: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let workId: String
    public let chapterId: String
    public let title: String
    public let summary: String?
    public let pov: String?
    public let status: String
    public let position: Int
    public let wordCount: Int
    public let contentHash: String
}

public struct OutlineChapter: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let workId: String
    public let title: String
    public let summary: String?
    public let position: Int
    public let scenes: [SceneSummary]
}

public struct Outline: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let title: String
    public let kind: WorkKind
    public let status: String
    public let wordCount: Int
    public let chapters: [OutlineChapter]
}

public struct SceneWithContent: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let workId: String
    public let title: String
    public let status: String
    public let wordCount: Int
    /// Carried back on the next save so the server can refuse a write whose
    /// base has moved, rather than silently overwriting another device.
    public let contentHash: String
    public let content: DocNode
}

public struct SaveResult: Codable, Equatable, Sendable {
    public let scene: SceneSummary
    public let revisionId: String?
    public let workWordCount: Int
}

public struct Version: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let workId: String
    public let number: Int
    public let label: String
    public let state: String
    public let wordCount: Int
    public let createdAt: String
    public let publishedAt: String?
    public let finalizedAt: String?
}

public struct VersionEntry: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let chapterId: String
    public let chapterTitle: String
    public let sceneId: String
    public let sceneTitle: String
    public let wordCount: Int
}

public struct VersionDetail: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let workId: String
    public let label: String
    public let state: String
    public let wordCount: Int
    public let entries: [VersionEntry]
}

public struct VersionContent: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let label: String
    public let wordCount: Int
    public let chapters: [ContentChapter]

    public struct ContentChapter: Codable, Identifiable, Equatable, Sendable {
        public let chapterId: String
        public let title: String
        public let position: Int
        public let scenes: [ContentScene]
        public var id: String { chapterId }
    }

    public struct ContentScene: Codable, Identifiable, Equatable, Sendable {
        public let sceneId: String
        public let title: String
        public let position: Int
        public let wordCount: Int
        public let content: DocNode
        public var id: String { sceneId }
    }
}

public struct TextChange: Codable, Equatable, Sendable {
    public enum Op: String, Codable, Sendable { case equal, insert, delete }
    public let op: Op
    public let value: String
}

public struct SceneDiff: Codable, Identifiable, Equatable, Sendable {
    public let sceneId: String
    public let sceneTitle: String
    public let chapterTitle: String
    public let kind: String
    public let wordsBefore: Int
    public let wordsAfter: Int
    public let changes: [TextChange]?
    public var id: String { sceneId }
}

public struct DiffSummary: Codable, Equatable, Sendable {
    public let scenesAdded: Int
    public let scenesRemoved: Int
    public let scenesModified: Int
    public let scenesMoved: Int
    public let wordsAdded: Int
    public let wordsRemoved: Int
}

public struct DiffResponse: Codable, Equatable, Sendable {
    public let from: Version
    public let to: Version
    public let summary: DiffSummary
    public let scenes: [SceneDiff]
}

public enum ReviewVisibility: String, Codable, CaseIterable, Hashable, Sendable {
    case unlisted, platform, `public`

    public var label: String {
        switch self {
        case .unlisted: "Anyone with the link"
        case .platform: "Signed-in writers"
        case .public: "Everyone"
        }
    }
}

public struct AuthorCard: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let displayName: String
}

public struct PublicationCard: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let workId: String
    public let versionId: String
    public let visibility: ReviewVisibility
    public let scope: String
    public let reviewPrompt: String?
    public let createdAt: String
    public let workTitle: String
    public let workKind: WorkKind
    public let author: AuthorCard
    public let versionLabel: String
    public let wordCount: Int
    public let commentCount: Int
    public let excerpt: String
}

public enum CommentCategory: String, Codable, CaseIterable, Sendable {
    case general, praise, plot, pacing, prose, character, typo
}

public struct Comment: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let publicationId: String
    public let sceneId: String?
    public let parentId: String?
    public let authorId: String
    public let body: String
    public let anchor: String?
    public let anchorQuote: String?
    public let category: CommentCategory
    public let state: String
    public let score: Int
    public let createdAt: String
}

public struct CommentView: Codable, Identifiable, Equatable, Sendable {
    public let id: String
    public let publicationId: String
    public let sceneId: String?
    public let parentId: String?
    public let authorId: String
    public let body: String
    public let anchor: String?
    public let anchorQuote: String?
    public let category: CommentCategory
    public let state: String
    public let score: Int
    public let createdAt: String
    public let author: AuthorCard
    public let viewerVote: Int
    public let replies: [CommentView]

    /// Decoded `{from,to}` pair, when the comment is anchored to a passage.
    public var anchorRange: (from: Int, to: Int)? {
        guard
            let anchor,
            let data = anchor.data(using: .utf8),
            let object = try? JSONDecoder().decode([String: Int].self, from: data),
            let from = object["from"], let to = object["to"]
        else { return nil }
        return (from, to)
    }
}

public struct VoteResult: Codable, Equatable, Sendable {
    public let commentId: String
    public let score: Int
    public let viewerVote: Int
}
