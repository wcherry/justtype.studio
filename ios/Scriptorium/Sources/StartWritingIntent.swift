import AppIntents
import ScriptoriumKit
import SwiftUI

/// "Start writing" — from Siri, the Shortcuts app, or the home screen.
///
/// The whole point is the gap between wanting to write and writing: a writer
/// with ten minutes should not spend one of them navigating. This opens the
/// app on the scene they had last, and nothing else.
struct StartWritingIntent: AppIntent {
    static var title: LocalizedStringResource = "Start writing"
    static var description = IntentDescription(
        "Opens the scene you were last working on."
    )
    /// The app has to come to the front: the point is to be writing.
    static var openAppWhenRun = true

    @MainActor
    func perform() async throws -> some IntentResult & ProvidesDialog {
        let snapshot = SharedState.load()
        Deeplink.shared.open = snapshot?.sceneId
        if let title = snapshot?.sceneTitle {
            return .result(dialog: IntentDialog("Opening \(title)."))
        }
        return .result(dialog: IntentDialog("Opening Just Type Studio."))
    }
}

/// Where an intent leaves what it wants the app to do.
///
/// An intent runs before the interface exists, so it cannot navigate. It can
/// leave a note.
@MainActor
final class Deeplink: ObservableObject {
    static let shared = Deeplink()
    @Published var open: String?
}

struct ScriptoriumShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: StartWritingIntent(),
            phrases: [
                "Start writing in \(.applicationName)",
                "Open my scene in \(.applicationName)",
            ],
            shortTitle: "Start writing",
            systemImageName: "pencil.line"
        )
    }
}
