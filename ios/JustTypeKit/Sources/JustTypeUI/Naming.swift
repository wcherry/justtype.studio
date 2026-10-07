import Foundation

/// A new chapter or scene is untitled. Wherever it is shown it is called by
/// where it falls — "Chapter 3", "Scene 2", counting scenes within their
/// chapter — so the name follows it when it moves, until the writer names it.
/// The web app and the server name them the same way.
enum Naming {
    static func isUntitled(_ title: String) -> Bool {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// `title`, or "Chapter `number`" (1-based) when there is none.
    static func chapter(_ title: String, number: Int) -> String {
        isUntitled(title) ? "Chapter \(number)" : title
    }

    /// `title`, or "Scene `number`" (1-based, within its chapter) when there is none.
    static func scene(_ title: String, number: Int) -> String {
        isUntitled(title) ? "Scene \(number)" : title
    }
}
