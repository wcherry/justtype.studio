import JustTypeKit
import SwiftUI
import JustTypeUI

/// The app target is deliberately thin: everything testable lives in the
/// JustTypeKit / JustTypeUI package, which builds and tests without a
/// simulator.
@main
struct JustTypeApp: App {
    @StateObject private var deeplink = Deeplink.shared
    /// A universal link from the daily prompt email, for the Prompts tab.
    @State private var link: URL?

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
                RootView(
                    baseURL: AppConfig.apiBaseURL,
                    startWriting: Binding(
                        get: {
                            deeplink.open.map {
                                OpenScene(
                                    id: $0,
                                    title: SharedState.load()?.sceneTitle ?? "Writing"
                                )
                            }
                        },
                        set: { if $0 == nil { deeplink.open = nil } }
                    ),
                    link: $link
                )
                .onOpenURL { url in
                    if PromptLink.handles(url) { link = url }
                }
                .onContinueUserActivity(NSUserActivityTypeBrowsingWeb) { activity in
                    if let url = activity.webpageURL, PromptLink.handles(url) { link = url }
                }
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
        return URL(string: "https://www.justtype.studio/api")!
    }
}
