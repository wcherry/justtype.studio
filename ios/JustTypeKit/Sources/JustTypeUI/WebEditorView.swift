import SwiftUI
import WebKit
import JustTypeKit

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
    /// Positions, the words between them, and the marks they carry.
    let onSelection: (Selection) -> Void
    /// Reading rather than writing: no caret, no keyboard, still selectable —
    /// which is what makes character-level anchoring work on a phone.
    var editable = true
    var typography: Typography?
    /// Which page of how many, in paged reading.
    var onPages: ((Int, Int) -> Void)?
    /// Loading failures, which are otherwise a blank white rectangle with
    /// nothing in it to say what went wrong.
    var onProblem: ((String) -> Void)?
    /// Formatting commands the toolbar sends down. Changing the value sends it.
    var command: Command?

    /// What the reader has their finger on.
    public struct Selection: Equatable, Sendable {
        public let from: Int
        public let to: Int
        public let quote: String
        public let marks: Set<String>

        public var isEmpty: Bool { from == to }
    }

    /// How the page should read. The defaults are the web client's.
    public struct Typography: Equatable {
        public var size: Double
        public var family: String
        public var leading: Double
        public var paged: Bool

        public init(size: Double = 17, family: String = "serif", leading: Double = 1.6, paged: Bool = false) {
            self.size = size
            self.family = family
            self.leading = leading
            self.paged = paged
        }

        var families: String {
            switch family {
            case "sans": "-apple-system, BlinkMacSystemFont, sans-serif"
            case "mono": "ui-monospace, SFMono-Regular, Menlo, monospace"
            default: "'Iowan Old Style', Georgia, serif"
            }
        }
    }

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
        editable: Bool = true,
        typography: Typography? = nil,
        onChange: @escaping (API.DocNode, Int) -> Void = { _, _ in },
        onSelection: @escaping (Selection) -> Void = { _ in },
        onPages: ((Int, Int) -> Void)? = nil,
        onProblem: ((String) -> Void)? = nil
    ) {
        self.document = document
        self.command = command
        self.editable = editable
        self.typography = typography
        self.onChange = onChange
        self.onSelection = onSelection
        self.onPages = onPages
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
        configuration.userContentController.add(context.coordinator, name: "justtype")
        // Served rather than loaded from a file URL — see EditorSchemeHandler
        // for what a file URL costs.
        configuration.setURLSchemeHandler(
            context.coordinator.scheme,
            forURLScheme: EditorSchemeHandler.scheme
        )
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

        webView.load(URLRequest(url: EditorSchemeHandler.pageURL))
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
            webView.evaluateJavaScript("window.justtype.toggle('\(command.name)')")
        }
        if context.coordinator.loaded, context.coordinator.lastEditable != editable {
            context.coordinator.lastEditable = editable
            webView.evaluateJavaScript("window.justtype.setEditable(\(editable))")
        }
        if let typography, context.coordinator.lastTypography != typography {
            context.coordinator.lastTypography = typography
            context.coordinator.apply(typography)
        }
    }

    public final class Coordinator: NSObject, WKScriptMessageHandler, WKNavigationDelegate {
        private let parent: WebEditorView
        weak var webView: WKWebView?
        private(set) var loaded = false
        private(set) var pushed: API.DocNode?
        let scheme = EditorSchemeHandler()
        var lastCommand: Command?
        var lastEditable: Bool?
        var lastTypography: Typography?

        func apply(_ typography: Typography) {
            let script = "window.justtype.setTypography({size: \(typography.size),"
                + " family: \"\(typography.families)\","
                + " leading: \(typography.leading), paged: \(typography.paged)})"
            webView?.evaluateJavaScript(script)
        }

        /// Turn a page. Reading is the one place a phone beats a laptop, and
        /// paging is most of why.
        public func turn(_ by: Int) {
            webView?.evaluateJavaScript("window.justtype.turn(\(by))")
        }

        init(_ parent: WebEditorView) {
            self.parent = parent
        }

        func push(_ document: API.DocNode) {
            guard
                let data = try? JSONEncoder().encode(document),
                let json = String(data: data, encoding: .utf8)
            else { return }
            pushed = document
            webView?.evaluateJavaScript("window.justtype.load(\(json))")
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
                    + " api: typeof window.justtype,"
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
                if !parent.editable {
                    lastEditable = false
                    webView?.evaluateJavaScript("window.justtype.setEditable(false)")
                }
                if let typography = parent.typography {
                    lastTypography = typography
                    apply(typography)
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
                    Selection(
                        from: body["from"] as? Int ?? 0,
                        to: body["to"] as? Int ?? 0,
                        quote: body["quote"] as? String ?? "",
                        marks: Set(body["marks"] as? [String] ?? [])
                    )
                )
            case "pages":
                parent.onPages?(body["page"] as? Int ?? 1, body["pages"] as? Int ?? 1)
            default:
                break
            }
        }
    }
}
