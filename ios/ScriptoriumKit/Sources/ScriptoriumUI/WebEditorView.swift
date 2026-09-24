import SwiftUI
import WebKit
import ScriptoriumKit

#if canImport(UIKit)
    import UIKit
    public typealias ViewRepresentable = UIViewRepresentable
#else
    import AppKit
    public typealias ViewRepresentable = NSViewRepresentable
#endif

/// The writing surface: the same TipTap the browser runs, hosted in a web view.
///
/// Chosen over an attributed-string model with a spike — see
/// `spikes/4.1-mobile-editing/README.md`. The reason that matters at runtime is
/// this: the document the page hands back *is* a ProseMirror document, and the
/// positions it reports are the ones the server and the browser use. There is
/// no second coordinate system to keep in step, so a comment anchored on a
/// phone highlights the same words in a browser.
///
/// The page is loaded from the app bundle, never from the network — a writer on
/// a plane has to be able to open a scene.
public struct WebEditorView: ViewRepresentable {
    let document: API.DocNode
    /// Fires on every edit, already in the shape the API takes.
    let onChange: (API.DocNode, Int) -> Void
    let onSelection: (Int, Int, Set<String>) -> Void
    /// Loading failures, which are otherwise a blank white rectangle with
    /// nothing in it to say what went wrong.
    var onProblem: ((String) -> Void)?
    /// Formatting commands the toolbar sends down. Changing the value sends it.
    var command: Command?

    public struct Command: Equatable {
        let name: String
        /// So that pressing bold twice sends two commands rather than one.
        let issue: Int

        public init(name: String, issue: Int) {
            self.name = name
            self.issue = issue
        }
    }

    public init(
        document: API.DocNode,
        command: Command? = nil,
        onChange: @escaping (API.DocNode, Int) -> Void,
        onSelection: @escaping (Int, Int, Set<String>) -> Void,
        onProblem: ((String) -> Void)? = nil
    ) {
        self.document = document
        self.command = command
        self.onChange = onChange
        self.onSelection = onSelection
        self.onProblem = onProblem
    }

    public func makeCoordinator() -> Coordinator { Coordinator(self) }

    #if canImport(UIKit)
        public func makeUIView(context: Context) -> WKWebView { makeWebView(context) }
        public func updateUIView(_ webView: WKWebView, context: Context) {
            update(webView, context)
        }
    #else
        // The package builds for macOS so the shared layer can be tested
        // without a simulator; the editor comes along rather than being
        // `#if`-ed out of existence, which would make it untestable there.
        public func makeNSView(context: Context) -> WKWebView { makeWebView(context) }
        public func updateNSView(_ webView: WKWebView, context: Context) {
            update(webView, context)
        }
    #endif

    private func makeWebView(_ context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.userContentController.add(context.coordinator, name: "scriptorium")
        // `limitsNavigationsToAppBoundDomains` belongs nowhere near this: with
        // no `WKAppBoundDomains` in the Info.plist it refuses every navigation,
        // file URLs included, and does it by rendering nothing. There is no
        // remote code here to restrict — the page ships in the app bundle.

        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = context.coordinator
        #if canImport(UIKit)
            webView.isOpaque = false
            webView.backgroundColor = .clear
            // A writer dragging down to see the paragraph above should not have
            // to dismiss the keyboard first.
            webView.scrollView.keyboardDismissMode = .interactive
        #endif
        context.coordinator.webView = webView

        guard
            let url = Bundle.module.url(
                forResource: "index",
                withExtension: "html",
                subdirectory: "Editor"
            )
        else {
            onProblem?("the editor is missing from the app bundle")
            return webView
        }
        webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
        return webView
    }

    private func update(_ webView: WKWebView, _ context: Context) {
        // Only push a document the page has not got: setContent resets the
        // selection, and doing it on every SwiftUI update would fight the
        // writer for the caret.
        if context.coordinator.loaded, context.coordinator.pushed != document {
            context.coordinator.push(document)
        }
        if let command, context.coordinator.lastCommand != command {
            context.coordinator.lastCommand = command
            webView.evaluateJavaScript("window.scriptorium.toggle('\(command.name)')")
        }
    }

    public final class Coordinator: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
        private let parent: WebEditorView
        weak var webView: WKWebView?
        private(set) var loaded = false
        private(set) var pushed: API.DocNode?
        var lastCommand: Command?

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

        public func webView(
            _ webView: WKWebView,
            didFailProvisionalNavigation navigation: WKNavigation!,
            withError error: Error
        ) {
            parent.onProblem?("the editor did not load: \(error.localizedDescription)")
        }

        public func webView(
            _ webView: WKWebView,
            didFail navigation: WKNavigation!,
            withError error: Error
        ) {
            parent.onProblem?("the editor failed: \(error.localizedDescription)")
        }

        /// The page is up but never said `ready`, which means the script threw.
        public func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) { [weak self] in
                guard let self, !self.loaded else { return }
                let probe = "JSON.stringify({error: window.__error || null,"
                    + " api: typeof window.scriptorium,"
                    + " editor: !!document.querySelector('.ProseMirror')})"
                webView.evaluateJavaScript(probe) { value, error in
                    let detail = (value as? String) ?? error?.localizedDescription ?? "no answer"
                    self.parent.onProblem?("the editor did not start: \(detail)")
                }
            }
        }

        public func userContentController(
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
                // The first paint can land before the view has its size, and
                // the page then draws a document laid out for a zero-width
                // window — the heading comes out invisible until something
                // forces a relayout. Nudging it once after the push is cheaper
                // than making the page watch for it.
                DispatchQueue.main.async { [weak self] in
                    self?.webView?.evaluateJavaScript(
                        "window.dispatchEvent(new Event('resize'))"
                    )
                }
            case "change":
                guard
                    let raw = body["doc"],
                    let data = try? JSONSerialization.data(withJSONObject: raw),
                    let document = try? JSONDecoder().decode(API.DocNode.self, from: data)
                else { return }
                // Remember what the page holds, so `updateUIView` does not hand
                // the writer's own edit back to them.
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
