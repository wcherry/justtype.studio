import SwiftUI
import ScriptoriumUI

/// The app target is deliberately thin: everything testable lives in the
/// ScriptoriumKit / ScriptoriumUI package, which builds and tests without a
/// simulator.
@main
struct ScriptoriumApp: App {
    var body: some SwiftUI.Scene {
        WindowGroup {
            // The 4.1 spike harness, reachable only with `-spike41` on the
            // command line. Delete this branch, `spikes/4.1-mobile-editing`
            // and the Editor resource together when the spike is acted on.
            if CommandLine.arguments.contains("-uitest") {
                EditorHarnessView(
                    reading: CommandLine.arguments.contains("-reading")
                )
            } else {
                RootView(baseURL: AppConfig.apiBaseURL)
            }
        }
    }
}

enum AppConfig {
    /// Overridable per build configuration via the `API_BASE_URL` Info.plist key,
    /// which project.yml wires to an xcconfig setting.
    static var apiBaseURL: URL {
        if let raw = Bundle.main.object(forInfoDictionaryKey: "API_BASE_URL") as? String,
           let url = URL(string: raw) {
            return url
        }
        // Simulator default: the backend running on the host machine.
        return URL(string: "http://127.0.0.1:8088/api")!
    }
}
