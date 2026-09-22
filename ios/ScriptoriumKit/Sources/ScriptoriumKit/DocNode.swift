import Foundation

/// Document behaviour, on the generated `API.DocNode`.
///
/// The shape is generated from the Rust contract; everything here is the
/// arithmetic and flattening the clients need, which no schema can express.

public extension API.DocNode {
    static let empty = API.DocNode(content: [API.DocNode(type: "paragraph")], type: "doc")

    /// Nodes that hold no content of their own and occupy exactly one position.
    static let leafTypes: Set<String> = ["horizontalRule", "hardBreak", "image"]

    var isLeaf: Bool { API.DocNode.leafTypes.contains(type) }

    /// ProseMirror node size. Text nodes measure in UTF-16 code units, which is
    /// what the web editor counts, so anchors agree across clients.
    var nodeSize: Int {
        if type == "text" { return text?.utf16.count ?? 0 }
        if isLeaf { return 1 }
        return contentSize + 2
    }

    var contentSize: Int {
        (content ?? []).reduce(0) { $0 + $1.nodeSize }
    }

    /// Flattened text, with one line per block, matching the server's
    /// `domain::doc::plain_text`.
    var plainText: String {
        var out = ""
        appendPlainText(into: &out)
        return out.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static let blockTypes: Set<String> = [
        "paragraph", "heading", "blockquote", "listItem", "codeBlock", "horizontalRule",
    ]

    private func appendPlainText(into out: inout String) {
        if type == "text" {
            out += text ?? ""
            return
        }
        if type == "hardBreak" {
            out += "\n"
            return
        }
        for child in content ?? [] {
            child.appendPlainText(into: &out)
        }
        if API.DocNode.blockTypes.contains(type), !out.hasSuffix("\n") {
            out += "\n"
        }
    }

    var wordCount: Int {
        plainText.split(whereSeparator: { $0.isWhitespace }).count
    }

    static func fromPlainText(_ text: String) -> API.DocNode {
        let paragraphs = text.components(separatedBy: "\n\n").map { paragraph -> API.DocNode in
            paragraph.trimmingCharacters(in: .whitespaces).isEmpty
                ? API.DocNode(type: "paragraph")
                : API.DocNode(content: [API.DocNode(text: paragraph, type: "text")], type: "paragraph")
        }
        return API.DocNode(content: paragraphs.isEmpty ? [API.DocNode(type: "paragraph")] : paragraphs, type: "doc")
    }

    /// True when the document carries formatting a plain-text editor cannot
    /// represent. The iOS editor refuses to open such a scene for editing
    /// rather than silently flattening the author's italics away.
    var hasRichFormatting: Bool {
        if let marks, !marks.isEmpty { return true }
        if type != "doc", type != "paragraph", type != "text" { return true }
        return (content ?? []).contains { $0.hasRichFormatting }
    }

    /// Walks the top level of a document and records where each block sits.
    func blocks() -> [DocBlock] {
        var result: [DocBlock] = []
        // Positions inside the doc node start at 0.
        var pos = 0
        for (index, child) in (content ?? []).enumerated() {
            let contentStart = child.isLeaf ? pos : pos + 1
            let contentEnd = child.isLeaf ? pos + 1 : contentStart + child.contentSize
            result.append(
                DocBlock(
                    id: index,
                    type: child.type,
                    level: child.attrs?.objectValue?["level"]?.intValue ?? 0,
                    text: child.plainText,
                    from: contentStart,
                    to: contentEnd
                )
            )
            pos += child.nodeSize
        }
        return result
    }
}

/// One top-level block of a document, with the ProseMirror positions it spans.
///
/// These positions are the same coordinates the web editor reports for a
/// selection, so a comment anchored from iOS lines up exactly with one anchored
/// from the browser.
public struct DocBlock: Identifiable, Equatable, Sendable {
    public let id: Int
    public let type: String
    public let level: Int
    public let text: String
    /// Inclusive start of the block's content.
    public let from: Int
    /// Exclusive end of the block's content.
    public let to: Int

    public var isEmpty: Bool { text.trimmingCharacters(in: .whitespaces).isEmpty }
}
