import SwiftUI
import WebKit
import ScriptoriumKit

/// **Spike — Phase 4.1, path B.** The host a WKWebView editor would need.
///
/// Not wired into the app. It is here so the cost of this path is a file
/// somebody can read rather than an estimate: the whole bridge is below, and
/// it is small, because the editor and the document model are the ones the web
/// client already uses.
///
/// The page is loaded from the app bundle, never from the network — a writer
/// on a plane has to be able to open a scene.
struct WebEditorView: UIViewRepresentable {
    let document: API.DocNode
    /// Fires on every edit, already in the shape the API takes.
    let onChange: (API.DocNode, Int) -> Void
    let onSelection: (Int, Int, Set<String>) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.userContentController.add(context.coordinator, name: "scriptorium")
        // No remote code, no remote data: everything this page needs ships
        // with the app.
        configuration.limitsNavigationsToAppBoundDomains = true

        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.scrollView.keyboardDismissMode = .interactive
        context.coordinator.webView = webView

        guard let url = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "Editor") else {
            assertionFailure("the editor bundle is missing from the app")
            return webView
        }
        webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        // Only push a document the page has not got: setContent resets the
        // selection, and doing it on every SwiftUI update would fight the
        // writer for the caret.
        guard context.coordinator.loaded, context.coordinator.pushed != document else { return }
        context.coordinator.push(document)
    }

    final class Coordinator: NSObject, WKScriptMessageHandler {
        private let parent: WebEditorView
        weak var webView: WKWebView?
        private(set) var loaded = false
        private(set) var pushed: API.DocNode?

        init(_ parent: WebEditorView) {
            self.parent = parent
        }

        func push(_ document: API.DocNode) {
            guard
                let data = try? JSONEncoder().encode(document),
                let json = String(data: data, encoding: .utf8)
            else { return }
            pushed = document
            webView?.evaluateJavaScript("window.scriptorium.load(\(json))")
        }

        func userContentController(
            _ controller: WKUserContentController,
            didReceive message: WKScriptMessage
        ) {
            guard
                let body = message.body as? [String: Any],
                let kind = body["kind"] as? String
            else { return }

            switch kind {
            case "ready":
                loaded = true
                push(parent.document)
            case "change":
                guard
                    let raw = body["doc"],
                    let data = try? JSONSerialization.data(withJSONObject: raw),
                    let document = try? JSONDecoder().decode(API.DocNode.self, from: data)
                else { return }
                // Remember what the page holds, so `updateUIView` does not
                // hand the writer's own edit back to them.
                pushed = document
                parent.onChange(document, body["words"] as? Int ?? 0)
            case "selection":
                parent.onSelection(
                    body["from"] as? Int ?? 0,
                    body["to"] as? Int ?? 0,
                    Set(body["marks"] as? [String] ?? [])
                )
            default:
                break
            }
        }
    }
}
