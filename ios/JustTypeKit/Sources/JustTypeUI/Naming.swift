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

    /// Every chapter's display name, in order. Front and back matter are not
    /// numbered and do not take a number from the chapters around them:
    /// "Chapter 4" is the fourth chapter, as on the web and in exports.
    static func chapters(_ chapters: [(title: String, role: String)]) -> [String] {
        var numbered = 0
        return chapters.map { chapter in
            switch chapter.role {
            case "front": return isUntitled(chapter.title) ? "Front matter" : chapter.title
            case "back": return isUntitled(chapter.title) ? "Back matter" : chapter.title
            default:
                numbered += 1
                return Self.chapter(chapter.title, number: numbered)
            }
        }
    }

    /// The badge shown beside front and back matter; nothing for a chapter.
    static func roleBadge(_ role: String) -> String? {
        switch role {
        case "front": return "Front matter"
        case "back": return "Back matter"
        default: return nil
        }
    }

    /// `title`, or "Scene `number`" (1-based, within its chapter) when there is none.
    static func scene(_ title: String, number: Int) -> String {
        isUntitled(title) ? "Scene \(number)" : title
    }
}
