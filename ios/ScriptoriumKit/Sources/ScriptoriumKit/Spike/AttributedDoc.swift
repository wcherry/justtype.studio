import Foundation

#if canImport(UIKit)
    import UIKit
    public typealias PlatformFont = UIFont
#else
    import AppKit
    public typealias PlatformFont = NSFont
#endif

/// **Spike — not wired into the editor.** Phase 4.1, path A.
///
/// A ProseMirror document as an `NSAttributedString`, and back. Written to
/// answer one question with code rather than opinion: what does a native
/// attributed-string editor lose?
///
/// The marks are carried in an attribute of our own rather than read back out
/// of the font. A bold serif face and a regular face with the bold trait are
/// the same picture and different data, and "was this bold, or is the font
/// just heavy" is not a question with an answer at save time. Owning the
/// attribute means owning every formatting command too — the system's own
/// bold shortcut would write a font trait this converter cannot see.
public enum AttributedDoc {
    public static let markKey = NSAttributedString.Key("scriptorium.marks")
    public static let blockKey = NSAttributedString.Key("scriptorium.block")
    public static let hrefKey = NSAttributedString.Key("scriptorium.href")

    /// The block types the converter can represent. Anything else is the
    /// answer to the spike's question.
    public enum Block: Equatable {
        case paragraph
        case heading(level: Int)
        case blockquote
        case codeBlock
        case listItem(ordered: Bool, depth: Int)
        case horizontalRule

        var nodeType: String {
            switch self {
            case .paragraph: "paragraph"
            case .heading: "heading"
            case .blockquote: "blockquote"
            case .codeBlock: "codeBlock"
            case .listItem: "listItem"
            case .horizontalRule: "horizontalRule"
            }
        }
    }

    // MARK: - Document to attributed string

    public static func attributedString(from doc: API.DocNode) -> NSAttributedString {
        let out = NSMutableAttributedString()
        for block in doc.content ?? [] {
            append(block, into: out, depth: 0)
        }
        // The trailing separator is a boundary, not a line of its own.
        if out.string.hasSuffix("\n") {
            out.deleteCharacters(in: NSRange(location: out.length - 1, length: 1))
        }
        return out
    }

    private static func append(_ node: API.DocNode, into out: NSMutableAttributedString, depth: Int) {
        switch node.type {
        case "bulletList", "orderedList":
            let ordered = node.type == "orderedList"
            for item in node.content ?? [] {
                for child in item.content ?? [] {
                    append(child, into: out, depth: depth, forcing: .listItem(ordered: ordered, depth: depth))
                }
            }
        case "blockquote":
            for child in node.content ?? [] {
                append(child, into: out, depth: depth, forcing: .blockquote)
            }
        default:
            append(node, into: out, depth: depth, forcing: nil)
        }
    }

    private static func append(
        _ node: API.DocNode,
        into out: NSMutableAttributedString,
        depth: Int,
        forcing block: Block?
    ) {
        let resolved: Block = block ?? {
            switch node.type {
            case "heading": .heading(level: node.attrs?.objectValue?["level"]?.intValue ?? 1)
            case "codeBlock": .codeBlock
            case "horizontalRule": .horizontalRule
            default: .paragraph
            }
        }()

        let line = NSMutableAttributedString()
        for child in node.content ?? [] {
            line.append(inline(child))
        }
        if resolved == .horizontalRule {
            line.append(NSAttributedString(string: "\u{2042}"))
        }
        line.addAttribute(
            blockKey,
            value: BlockBox(resolved),
            range: NSRange(location: 0, length: line.length)
        )
        out.append(line)
        out.append(NSAttributedString(string: "\n", attributes: [blockKey: BlockBox(resolved)]))
    }

    private static func inline(_ node: API.DocNode) -> NSAttributedString {
        if node.type == "hardBreak" {
            return NSAttributedString(string: "\u{2028}")
        }
        guard node.type == "text", let text = node.text else {
            // A nested inline node this converter does not know: keep its text
            // rather than dropping it, and lose only the wrapper.
            let out = NSMutableAttributedString()
            for child in node.content ?? [] { out.append(inline(child)) }
            return out
        }
        var attributes: [NSAttributedString.Key: Any] = [:]
        let marks = (node.marks ?? []).map(\.type)
        if !marks.isEmpty { attributes[markKey] = Set(marks) }
        if let href = (node.marks ?? [])
            .first(where: { $0.type == "link" })?
            .attrs?.objectValue?["href"]?.stringValue
        {
            attributes[hrefKey] = href
        }
        return NSAttributedString(string: text, attributes: attributes)
    }

    // MARK: - Attributed string back to a document

    public static func document(from attributed: NSAttributedString) -> API.DocNode {
        var blocks: [API.DocNode] = []
        var pending: [(Block, NSAttributedString)] = []

        attributed.enumerateLine { line, block in
            pending.append((block, line))
        }

        var index = 0
        while index < pending.count {
            let (block, line) = pending[index]
            switch block {
            case .listItem(let ordered, _):
                // Consecutive items of the same kind are one list, which is
                // how ProseMirror models them and not how a flat string does.
                var items: [API.DocNode] = []
                while index < pending.count,
                      case .listItem(let nextOrdered, _) = pending[index].0,
                      nextOrdered == ordered
                {
                    items.append(
                        API.DocNode(
                            content: [
                                API.DocNode(content: spans(of: pending[index].1), type: "paragraph")
                            ],
                            type: "listItem"
                        )
                    )
                    index += 1
                }
                blocks.append(
                    API.DocNode(content: items, type: ordered ? "orderedList" : "bulletList")
                )
                continue
            case .blockquote:
                var paragraphs: [API.DocNode] = []
                while index < pending.count, pending[index].0 == .blockquote {
                    paragraphs.append(
                        API.DocNode(content: spans(of: pending[index].1), type: "paragraph")
                    )
                    index += 1
                }
                blocks.append(API.DocNode(content: paragraphs, type: "blockquote"))
                continue
            case .horizontalRule:
                blocks.append(API.DocNode(type: "horizontalRule"))
            case .heading(let level):
                blocks.append(
                    API.DocNode(
                        attrs: .object(["level": .number(Double(level))]),
                        content: spans(of: line),
                        type: "heading"
                    )
                )
            case .codeBlock:
                blocks.append(API.DocNode(content: spans(of: line), type: "codeBlock"))
            case .paragraph:
                blocks.append(API.DocNode(content: spans(of: line), type: "paragraph"))
            }
            index += 1
        }

        return API.DocNode(content: blocks.isEmpty ? [API.DocNode(type: "paragraph")] : blocks, type: "doc")
    }

    private static func spans(of line: NSAttributedString) -> [API.DocNode]? {
        guard line.length > 0 else { return nil }
        var out: [API.DocNode] = []
        line.enumerateAttributes(in: NSRange(location: 0, length: line.length)) { attributes, range, _ in
            let text = (line.string as NSString).substring(with: range)
            if text == "\u{2028}" {
                out.append(API.DocNode(type: "hardBreak"))
                return
            }
            let marks = (attributes[markKey] as? Set<String>) ?? []
            let href = attributes[hrefKey] as? String
            let nodes: [API.DocMark]? = marks.isEmpty
                ? nil
                : marks.sorted().map { type in
                    type == "link" && href != nil
                        ? API.DocMark(attrs: .object(["href": .string(href!)]), type: type)
                        : API.DocMark(type: type)
                }
            out.append(API.DocNode(marks: nodes, text: text, type: "text"))
        }
        return out.isEmpty ? nil : out
    }
}

/// `NSAttributedString` attributes must be class types to survive copying, so
/// the block enum travels in a box.
private final class BlockBox: NSObject {
    let block: AttributedDoc.Block
    init(_ block: AttributedDoc.Block) { self.block = block }
    override func isEqual(_ object: Any?) -> Bool {
        (object as? BlockBox)?.block == block
    }
    override var hash: Int { String(describing: block).hashValue }
}

private extension NSAttributedString {
    /// Walks the string a line at a time, handing back each line's block kind.
    func enumerateLine(_ body: (NSAttributedString, AttributedDoc.Block) -> Void) {
        let text = string as NSString
        var start = 0
        while start <= text.length {
            let range = text.range(of: "\n", range: NSRange(location: start, length: text.length - start))
            let end = range.location == NSNotFound ? text.length : range.location
            let lineRange = NSRange(location: start, length: end - start)
            let attributes = length > 0
                ? self.attributes(at: min(start, length - 1), effectiveRange: nil)
                : [:]
            let block = (attributes[AttributedDoc.blockKey] as? BlockBox)?.block ?? .paragraph
            body(attributedSubstring(from: lineRange), block)
            if range.location == NSNotFound { return }
            start = end + 1
        }
    }
}
