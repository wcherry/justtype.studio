//! A scene as Markdown, and Markdown as a scene.
//!
//! Writing a document out is lossless for everything the editor can make —
//! paragraphs, headings, quotes, lists, code, rules, line breaks, and the bold,
//! italic, strike, code and underline marks. Reading one in is as generous as a
//! Markdown parser can be, so a file written by hand or by another tool opens
//! as a scene too; anything the editor has no node for (tables, images, links)
//! is kept as its text.
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde_json::{json, Value};

// ── TipTap → Markdown ───────────────────────────────────────────────────────

/// One scene's document as Markdown, ending in a single newline.
pub fn write_doc(doc: &Value) -> String {
    let mut out = String::new();
    blocks(doc, &mut out);
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

fn children(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn kind(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

/// Every child block of `node`, each followed by a blank line.
fn blocks(node: &Value, out: &mut String) {
    for child in children(node) {
        match kind(child) {
            "paragraph" => {
                let text = inline(child);
                if !text.trim().is_empty() {
                    out.push_str(&text);
                    out.push_str("\n\n");
                }
            }
            "heading" => {
                let level = child
                    .get("attrs")
                    .and_then(|a| a.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(2)
                    .clamp(1, 6) as usize;
                out.push_str(&format!("{} {}\n\n", "#".repeat(level), inline(child)));
            }
            "blockquote" => {
                let mut inner = String::new();
                blocks(child, &mut inner);
                for line in inner.trim_end().lines() {
                    if line.is_empty() {
                        out.push_str(">\n");
                    } else {
                        out.push_str(&format!("> {line}\n"));
                    }
                }
                out.push('\n');
            }
            "bulletList" => list(child, false, out),
            "orderedList" => list(child, true, out),
            "codeBlock" => {
                let text: String = children(child)
                    .iter()
                    .filter_map(|t| t.get("text").and_then(Value::as_str))
                    .collect();
                // A fence one backtick longer than any run inside the code.
                let longest = text
                    .split(|c| c != '`')
                    .map(str::len)
                    .max()
                    .unwrap_or(0)
                    .max(2);
                let fence = "`".repeat(longest + 1);
                out.push_str(&format!("{fence}\n{text}\n{fence}\n\n"));
            }
            "horizontalRule" => out.push_str("* * *\n\n"),
            _ => {}
        }
    }
}

fn list(node: &Value, ordered: bool, out: &mut String) {
    let start = node
        .get("attrs")
        .and_then(|a| a.get("start"))
        .and_then(Value::as_u64)
        .unwrap_or(1);
    for (index, item) in children(node).iter().enumerate() {
        let marker = if ordered {
            format!("{}. ", start + index as u64)
        } else {
            "- ".to_string()
        };
        let mut inner = String::new();
        blocks(item, &mut inner);
        let indent = " ".repeat(marker.len());
        for (line_no, line) in inner.trim_end().lines().enumerate() {
            if line_no == 0 {
                out.push_str(&marker);
            } else if !line.is_empty() {
                out.push_str(&indent);
            }
            out.push_str(line);
            out.push('\n');
        }
        if inner.trim().is_empty() {
            out.push_str(marker.trim_end());
            out.push('\n');
        }
    }
    out.push('\n');
}

/// The text of a textblock, with its marks.
fn inline(node: &Value) -> String {
    let mut out = String::new();
    for child in children(node) {
        match kind(child) {
            "text" => out.push_str(&marked(child)),
            "hardBreak" => out.push_str("  \n"),
            _ => {}
        }
    }
    out
}

fn marked(node: &Value) -> String {
    let text = node.get("text").and_then(Value::as_str).unwrap_or("");
    let has = |name: &str| {
        node.get("marks")
            .and_then(Value::as_array)
            .is_some_and(|m| m.iter().any(|m| kind(m) == name))
    };
    if has("code") {
        let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(longest + 1);
        let pad = if text.starts_with('`') || text.ends_with('`') {
            " "
        } else {
            ""
        };
        return format!("{fence}{pad}{text}{pad}{fence}");
    }
    // Space inside an emphasis marker would make it not one, so the marker
    // wraps the words and the spaces stay outside it.
    let core = text.trim();
    if core.is_empty() {
        return text.to_string();
    }
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let mut body = escape(core);
    if has("underline") {
        body = format!("<u>{body}</u>");
    }
    if has("strike") {
        body = format!("~~{body}~~");
    }
    if has("italic") {
        body = format!("*{body}*");
    }
    if has("bold") {
        body = format!("**{body}**");
    }
    format!("{lead}{body}{trail}")
}

/// Backslash-escapes what Markdown would otherwise read as formatting.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        match ch {
            '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            // Only a start of line makes these a heading, quote, list or rule.
            '#' | '>' | '+' | '-' if index == 0 => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    // "1. " at the start would become a list.
    let digits = out.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && out[digits..].starts_with(['.', ')']) {
        out.insert(digits, '\\');
    }
    out
}

// ── Markdown → TipTap ───────────────────────────────────────────────────────

/// A document from Markdown (or plain text, which is Markdown without any
/// formatting in it).
pub fn parse(markdown: &str) -> Value {
    let mut build = Builder::default();
    let parser = Parser::new_ext(markdown, Options::ENABLE_STRIKETHROUGH);
    for event in parser {
        build.event(event);
    }
    build.finish()
}

struct Open {
    /// Opened by the builder for text with no paragraph tag around it.
    implicit: bool,
    kind: &'static str,
    attrs: Option<Value>,
    content: Vec<Value>,
}

#[derive(Default)]
struct Builder {
    stack: Vec<Open>,
    root: Vec<Value>,
    bold: u32,
    italic: u32,
    strike: u32,
    underline: bool,
    in_code_block: bool,
}

impl Builder {
    fn open(&mut self, kind: &'static str, attrs: Option<Value>) {
        self.stack.push(Open {
            implicit: false,
            kind,
            attrs,
            content: Vec::new(),
        });
    }

    fn close(&mut self) {
        let Some(done) = self.stack.pop() else { return };
        let mut node = json!({ "type": done.kind });
        if let Some(attrs) = done.attrs {
            node["attrs"] = attrs;
        }
        // The editor has no empty paragraphs to keep: they are the writer
        // pressing return, which Markdown spells as a blank line anyway.
        let textblock = matches!(done.kind, "paragraph" | "heading");
        if textblock && done.content.is_empty() {
            return;
        }
        if textblock || !done.content.is_empty() || done.kind == "horizontalRule" {
            node["content"] = Value::Array(done.content);
        }
        if done.kind == "horizontalRule" {
            node.as_object_mut().map(|o| o.remove("content"));
        }
        self.push_block(node);
    }

    fn push_block(&mut self, node: Value) {
        match self.stack.last_mut() {
            Some(parent) => parent.content.push(node),
            None => self.root.push(node),
        }
    }

    /// Text that arrives straight inside a tight list item has no paragraph
    /// tags around it in the event stream; give it one.
    fn ensure_textblock(&mut self) {
        let needs = matches!(
            self.stack.last().map(|o| o.kind),
            Some("listItem" | "blockquote") | None
        );
        if needs {
            self.open("paragraph", None);
            if let Some(top) = self.stack.last_mut() {
                top.implicit = true;
            }
        }
    }

    /// Ends a paragraph made by `ensure_textblock` when something else begins.
    fn settle(&mut self) {
        if self.stack.last().is_some_and(|o| o.implicit) {
            self.close();
        }
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.in_code_block {
            if let Some(top) = self.stack.last_mut() {
                match top.content.last_mut() {
                    Some(last) => {
                        let joined = format!("{}{text}", last["text"].as_str().unwrap_or(""));
                        last["text"] = Value::String(joined);
                    }
                    None => top.content.push(json!({ "type": "text", "text": text })),
                }
            }
            return;
        }
        self.ensure_textblock();
        let mut marks: Vec<Value> = Vec::new();
        if self.bold > 0 {
            marks.push(json!({ "type": "bold" }));
        }
        if self.italic > 0 {
            marks.push(json!({ "type": "italic" }));
        }
        if self.strike > 0 {
            marks.push(json!({ "type": "strike" }));
        }
        if self.underline {
            marks.push(json!({ "type": "underline" }));
        }
        self.push_text(text, marks);
    }

    fn push_text(&mut self, text: &str, marks: Vec<Value>) {
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        let marks_value = if marks.is_empty() {
            None
        } else {
            Some(Value::Array(marks))
        };
        if let Some(last) = top.content.last_mut() {
            if last["type"] == "text" && last.get("marks").cloned() == marks_value {
                let joined = format!("{}{text}", last["text"].as_str().unwrap_or(""));
                last["text"] = Value::String(joined);
                return;
            }
        }
        let mut node = json!({ "type": "text", "text": text });
        if let Some(marks) = marks_value {
            node["marks"] = marks;
        }
        top.content.push(node);
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    self.settle();
                    self.open("paragraph", None);
                }
                Tag::Heading { level, .. } => {
                    self.settle();
                    self.open("heading", Some(json!({ "level": level as u8 })));
                }
                Tag::BlockQuote(_) => {
                    self.settle();
                    self.open("blockquote", None);
                }
                Tag::CodeBlock(kind) => {
                    self.settle();
                    let lang = match kind {
                        CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                            Some(json!({ "language": lang.to_string() }))
                        }
                        _ => None,
                    };
                    self.open("codeBlock", lang);
                    self.in_code_block = true;
                }
                Tag::List(start) => {
                    self.settle();
                    match start {
                        Some(n) => self.open("orderedList", Some(json!({ "start": n }))),
                        None => self.open("bulletList", None),
                    }
                }
                Tag::Item => self.open("listItem", None),
                Tag::Emphasis => self.italic += 1,
                Tag::Strong => self.bold += 1,
                Tag::Strikethrough => self.strike += 1,
                // Links, images, tables and the rest: their text still flows.
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) => self.close(),
                TagEnd::BlockQuote(_) | TagEnd::Item => {
                    self.settle();
                    self.close();
                }
                TagEnd::CodeBlock => {
                    self.in_code_block = false;
                    self.close();
                }
                TagEnd::List(_) => {
                    self.settle();
                    self.close();
                }
                TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
                TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
                TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
                _ => {}
            },
            Event::Text(text) => self.text(&text),
            Event::Code(text) => {
                self.ensure_textblock();
                self.push_text(&text, vec![json!({ "type": "code" })]);
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => {
                self.ensure_textblock();
                if let Some(top) = self.stack.last_mut() {
                    top.content.push(json!({ "type": "hardBreak" }));
                }
            }
            Event::Rule => {
                self.settle();
                self.push_block(json!({ "type": "horizontalRule" }));
            }
            Event::InlineHtml(html) | Event::Html(html) => {
                let tag = html.trim().to_ascii_lowercase();
                match tag.as_str() {
                    "<u>" => self.underline = true,
                    "</u>" => self.underline = false,
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Value {
        while !self.stack.is_empty() {
            self.close();
        }
        json!({ "type": "doc", "content": self.root })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> Value {
        json!({"type":"paragraph","content":[{"type":"text","text":text}]})
    }

    #[test]
    fn marks_survive_the_trip() {
        let doc = json!({"type":"doc","content":[{"type":"paragraph","content":[
            {"type":"text","text":"She said "},
            {"type":"text","text":"nothing","marks":[{"type":"italic"}]},
            {"type":"text","text":" and "},
            {"type":"text","text":"left","marks":[{"type":"bold"},{"type":"underline"}]},
            {"type":"text","text":"."}
        ]}]});
        let md = write_doc(&doc);
        assert_eq!(md, "She said *nothing* and **<u>left</u>**.\n");
        let back = parse(&md);
        let spans = back["content"][0]["content"].as_array().unwrap();
        assert_eq!(spans.len(), 5);
        assert_eq!(spans[1]["marks"][0]["type"], "italic");
        let marks: Vec<&str> = spans[3]["marks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["type"].as_str().unwrap())
            .collect();
        assert!(marks.contains(&"bold") && marks.contains(&"underline"));
    }

    #[test]
    fn blocks_survive_the_trip() {
        let doc = json!({"type":"doc","content":[
            {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Part one"}]},
            para("Opening."),
            {"type":"blockquote","content":[para("A quoted line.")]},
            {"type":"bulletList","content":[
                {"type":"listItem","content":[para("one")]},
                {"type":"listItem","content":[para("two")]}
            ]},
            {"type":"horizontalRule"},
            para("After the break.")
        ]});
        let back = parse(&write_doc(&doc));
        let kinds: Vec<&str> = back["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "heading",
                "paragraph",
                "blockquote",
                "bulletList",
                "horizontalRule",
                "paragraph"
            ]
        );
        assert_eq!(back["content"][0]["attrs"]["level"], 2);
        assert_eq!(back["content"][3]["content"].as_array().unwrap().len(), 2);
        assert_eq!(
            back["content"][3]["content"][1]["content"][0]["content"][0]["text"],
            "two"
        );
    }

    #[test]
    fn what_looks_like_formatting_is_escaped_and_comes_back_as_text() {
        let text = "2*3 = 6, a_b, # not a heading, [x] <u>";
        let doc =
            json!({"type":"doc","content":[para(text), para("1. not a list"), para("- nor this")]});
        let md = write_doc(&doc);
        let back = parse(&md);
        assert_eq!(back["content"].as_array().unwrap().len(), 3);
        assert_eq!(back["content"][0]["content"][0]["text"], text);
        assert_eq!(back["content"][1]["content"][0]["text"], "1. not a list");
        assert_eq!(back["content"][2]["content"][0]["text"], "- nor this");
    }

    #[test]
    fn line_breaks_and_paragraphs() {
        let md = "First line  \nsecond line\n\nNew paragraph.\n";
        let doc = parse(md);
        assert_eq!(doc["content"].as_array().unwrap().len(), 2);
        assert_eq!(doc["content"][0]["content"][1]["type"], "hardBreak");
    }

    #[test]
    fn plain_text_is_markdown_without_formatting() {
        let doc = parse("Just words.\n\nMore words.");
        assert_eq!(doc["content"].as_array().unwrap().len(), 2);
        assert_eq!(doc["content"][1]["content"][0]["text"], "More words.");
    }

    #[test]
    fn an_empty_document_is_an_empty_file() {
        assert_eq!(write_doc(&json!({"type":"doc","content":[]})), "");
        assert_eq!(parse("")["content"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn tight_lists_keep_their_items() {
        let doc = parse("1. one\n2. two\n3. three\n");
        let items = doc["content"][0]["content"].as_array().unwrap();
        assert_eq!(doc["content"][0]["type"], "orderedList");
        assert_eq!(items.len(), 3);
        assert_eq!(items[2]["content"][0]["content"][0]["text"], "three");
    }
}
