import Foundation

/// Behaviour on the generated API types.
///
/// `Generated/Schemas.swift` owns every field; this file owns everything that
/// is not a field. Keeping them apart means regenerating never clobbers hand
/// -written code, and hand-written code never quietly redefines the contract.

public extension API.WorkKind {
    var label: String {
        switch self {
        case .novel: "Novel"
        case .novella: "Novella"
        case .short_story: "Short story"
        case .serial: "Serial"
        }
    }
}

public extension API.OutlineResponse {
    /// Whether the caller may change this chapter's text.
    ///
    /// The server decides; this keeps the phone from offering a caret it would
    /// refuse, and from queueing saves that could never go up. An outline
    /// cached before collaboration existed carries no access, and then every
    /// work had one person on it — its owner.
    func canWrite(chapterId: String) -> Bool {
        guard let access else { return true }
        switch access.role {
        case .owner: return true
        case .writer: return access.chapterIds?.contains(chapterId) ?? true
        case .editor, .reader: return false
        }
    }
}

public extension API.Visibility {
    var label: String {
        switch self {
        case .unlisted: "Anyone with the link"
        case .platform: "Public"
        case .invited: "Named readers only"
        case .public: "Open to all"
        }
    }
}

public extension API.CommentView {
    /// Decoded `{from,to}` pair, when the comment is anchored to a passage.
    var anchorRange: (from: Int, to: Int)? {
        guard
            let anchor,
            let data = anchor.data(using: .utf8),
            let object = try? JSONDecoder().decode([String: Int].self, from: data),
            let from = object["from"], let to = object["to"]
        else { return nil }
        return (from, to)
    }
}

// Identifiable is what SwiftUI lists and navigation want; the generated types
// stay free of UI protocols.
extension API.Work: Identifiable {}
extension API.Scene: Identifiable {}
extension API.Chapter: Identifiable {}
extension API.OutlineChapter: Identifiable {}
extension API.SceneWithContent: Identifiable {}
extension API.SceneRevision: Identifiable {}
extension API.Version: Identifiable {}
extension API.VersionDetail: Identifiable {}
extension API.VersionEntry: Identifiable {}
extension API.PublicationCard: Identifiable {}
extension API.Comment: Identifiable {}
extension API.CommentView: Identifiable {}
extension API.AuthorCard: Identifiable {}
extension API.TemplateView: Identifiable {}
extension API.User: Identifiable {}
extension API.SceneDiff: Identifiable {
    public var id: String { sceneId }
}
extension API.ContentChapter: Identifiable {
    public var id: String { chapterId }
}
extension API.ContentScene: Identifiable {
    public var id: String { sceneId }
}
