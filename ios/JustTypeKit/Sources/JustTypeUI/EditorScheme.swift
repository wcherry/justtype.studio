import Foundation
import WebKit

/// Serves the editor out of the app bundle under a scheme of its own.
///
/// A file URL has an opaque origin, so WKWebView refuses to load a module
/// script from one — and says nothing about why. The workaround of folding the
/// bundle into the HTML as a classic script works until it doesn't: wrapping
/// 300 KB of minified JavaScript in a `try` block turns its top-level `const`s
/// into block-scoped ones, and on iOS 17 that is a temporal-dead-zone error
/// (`Can't find variable: An`) a hundred kilobytes from anything a person
/// wrote. It ran perfectly on iOS 26, which is the worst way to have a bug.
///
/// A custom scheme gives the page a real origin. The build output ships
/// exactly as Vite produced it, module script and all, and there is no text
/// surgery on minified code anywhere in the pipeline.
final class EditorSchemeHandler: NSObject, WKURLSchemeHandler {
    static let scheme = "justtype"
    static let pageURL = URL(string: "justtype://editor/index.html")!

    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        guard
            let url = task.request.url,
            let data = Self.data(for: url)
        else {
            task.didFailWithError(URLError(.fileDoesNotExist))
            return
        }

        let response = HTTPURLResponse(
            url: url,
            statusCode: 200,
            httpVersion: "HTTP/1.1",
            headerFields: [
                "Content-Type": Self.mime(for: url),
                "Content-Length": String(data.count),
                // Same-origin only: nothing here should ever be fetched by
                // anything but this page.
                "Access-Control-Allow-Origin": "null",
            ]
        )!
        task.didReceive(response)
        task.didReceive(data)
        task.didFinish()
    }

    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}

    /// The path under `Editor/`, with anything that could climb out of it
    /// removed. The requests all come from a page we shipped, but a resource
    /// loader is not the place to take that on trust.
    private static func data(for url: URL) -> Data? {
        let parts = url.path.split(separator: "/").filter { $0 != ".." && $0 != "." }
        guard !parts.isEmpty, let editor = Bundle.module.url(forResource: "Editor", withExtension: nil)
        else { return nil }
        let file = parts.reduce(editor) { $0.appendingPathComponent(String($1)) }
        return try? Data(contentsOf: file)
    }

    private static func mime(for url: URL) -> String {
        switch url.pathExtension {
        case "html": "text/html; charset=utf-8"
        case "js": "text/javascript; charset=utf-8"
        case "css": "text/css; charset=utf-8"
        case "json": "application/json"
        case "svg": "image/svg+xml"
        default: "application/octet-stream"
        }
    }
}
