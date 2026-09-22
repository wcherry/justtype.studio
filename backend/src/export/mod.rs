//! Turning a frozen version into something you can send someone.
//!
//! One assembled `Manuscript` feeds every writer, so the formats cannot disagree
//! about what the book says — only about how it looks. The TipTap JSON is
//! flattened into a small block model here, once, rather than in three places.

pub mod docx;
pub mod epub;
pub mod pdf;
pub mod text;

use serde_json::Value;

/// A run of text with the marks that survive export.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub text: String,
    pub italic: bool,
    pub bold: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Span>),
    Heading {
        level: u8,
        spans: Vec<Span>,
    },
    Quote(Vec<Span>),
    /// A break *within* a chapter. Rendered as `#` in manuscript format, a
    /// glyph in a book, and an `<hr/>` in EPUB.
    SceneBreak,
}

#[derive(Debug, Clone)]
pub struct SceneContent {
    /// Carried but not printed: scene titles are the writer's scaffolding, not
    /// the reader's. Kept because an outline export would want them.
    #[allow(dead_code)]
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct ChapterContent {
    pub title: String,
    pub scenes: Vec<SceneContent>,
}

#[derive(Debug, Clone, Default)]
pub struct Contact {
    pub legal_name: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Manuscript {
    pub title: String,
    pub subtitle: Option<String>,
    /// The name on the byline — a pen name when there is one.
    pub byline: String,
    /// Used for the running header in manuscript format.
    pub surname: String,
    /// The header's short title, so a long one does not wrap across the page.
    pub short_title: String,
    pub contact: Contact,
    pub copyright_line: Option<String>,
    pub dedication: Option<String>,
    pub epigraph: Option<String>,
    pub epigraph_source: Option<String>,
    pub word_count: i32,
    pub chapters: Vec<ChapterContent>,
}

/// Which set of conventions to lay the text out under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Shunn standard manuscript format: Courier 12pt, double spaced, indented
    /// paragraphs, `#` scene breaks, contact block, running header. What a
    /// submission to a magazine or an agent is expected to look like.
    Manuscript,
    /// Something closer to a finished book: proportional type, single spaced,
    /// a glyph for scene breaks, chapter title pages.
    Book,
}

impl Manuscript {
    /// Shunn asks for a rounded count — the point is an estimate of length, and
    /// a figure like 78,412 implies a precision nobody wants. Nearest hundred
    /// for a short story, nearest five hundred for a long one, nearest thousand
    /// for anything novel-length.
    pub fn rounded_word_count(&self) -> i32 {
        let words = self.word_count.max(0);
        let step = if words < 1_500 {
            100
        } else if words < 10_000 {
            500
        } else {
            1_000
        };
        (words + step / 2) / step * step
    }

    pub fn is_empty(&self) -> bool {
        self.chapters
            .iter()
            .all(|c| c.scenes.iter().all(|s| s.blocks.is_empty()))
    }
}

/// Marks TipTap applies that this model keeps. Anything else — a comment
/// highlight, a colour — is presentation the exported file has no business
/// carrying.
fn spans_from(node: &Value) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    collect_spans(node, &Span::default(), &mut spans);
    // Adjacent runs with identical formatting come back as one, so a word split
    // across two text nodes does not become two runs in the output.
    let mut merged: Vec<Span> = Vec::new();
    for span in spans {
        match merged.last_mut() {
            Some(last)
                if last.italic == span.italic
                    && last.bold == span.bold
                    && last.underline == span.underline =>
            {
                last.text.push_str(&span.text)
            }
            _ => merged.push(span),
        }
    }
    merged
}

fn collect_spans(node: &Value, inherited: &Span, out: &mut Vec<Span>) {
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");

    if node_type == "text" {
        let mut span = inherited.clone();
        for mark in node
            .get("marks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match mark.get("type").and_then(Value::as_str) {
                Some("italic" | "em") => span.italic = true,
                Some("bold" | "strong") => span.bold = true,
                Some("underline") => span.underline = true,
                _ => {}
            }
        }
        span.text = node
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if !span.text.is_empty() {
            out.push(span);
        }
        return;
    }

    if node_type == "hardBreak" {
        out.push(Span {
            text: "\n".into(),
            ..inherited.clone()
        });
        return;
    }

    for child in node
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        collect_spans(child, inherited, out);
    }
}

/// Flattens one scene's TipTap document into blocks.
pub fn blocks_from_doc(doc: &Value) -> Vec<Block> {
    let mut blocks = Vec::new();
    for node in doc
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match node.get("type").and_then(Value::as_str) {
            Some("paragraph") => {
                let spans = spans_from(node);
                // Empty paragraphs are the writer pressing return, not content.
                // Carrying them through would double-space an already
                // double-spaced manuscript.
                if !spans.is_empty() {
                    blocks.push(Block::Paragraph(spans));
                }
            }
            Some("heading") => {
                let level = node
                    .get("attrs")
                    .and_then(|a| a.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(2) as u8;
                blocks.push(Block::Heading {
                    level,
                    spans: spans_from(node),
                });
            }
            Some("blockquote") => blocks.push(Block::Quote(spans_from(node))),
            Some("horizontalRule") => blocks.push(Block::SceneBreak),
            Some("bulletList" | "orderedList") => {
                // Lists are rare in fiction and have no manuscript convention;
                // each item becomes its own paragraph rather than being lost.
                for item in node
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let spans = spans_from(item);
                    if !spans.is_empty() {
                        blocks.push(Block::Paragraph(spans));
                    }
                }
            }
            _ => {}
        }
    }
    blocks
}

/// Escapes text for XML bodies — used by both the DOCX and EPUB writers.
pub(crate) fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(value: Value) -> Vec<Block> {
        blocks_from_doc(&value)
    }

    #[test]
    fn marks_become_spans() {
        let blocks = doc(json!({"type":"doc","content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"She said "},
                {"type":"text","text":"nothing","marks":[{"type":"italic"}]},
                {"type":"text","text":" at all."}
            ]}
        ]}));
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected a paragraph")
        };
        assert_eq!(spans.len(), 3);
        assert!(spans[1].italic);
        assert!(!spans[0].italic);
    }

    #[test]
    fn adjacent_runs_with_the_same_marks_merge() {
        let blocks = doc(json!({"type":"doc","content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"one "},
                {"type":"text","text":"two"}
            ]}
        ]}));
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!()
        };
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "one two");
    }

    #[test]
    fn empty_paragraphs_are_dropped() {
        let blocks = doc(json!({"type":"doc","content":[
            {"type":"paragraph"},
            {"type":"paragraph","content":[{"type":"text","text":"Real."}]},
            {"type":"paragraph","content":[]}
        ]}));
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn horizontal_rules_are_scene_breaks() {
        let blocks = doc(json!({"type":"doc","content":[{"type":"horizontalRule"}]}));
        assert_eq!(blocks, vec![Block::SceneBreak]);
    }

    #[test]
    fn unknown_marks_are_ignored_not_dropped() {
        let blocks = doc(json!({"type":"doc","content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"kept","marks":[{"type":"highlight"}]}
            ]}
        ]}));
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!()
        };
        assert_eq!(spans[0].text, "kept");
        assert!(!spans[0].italic);
    }

    fn manuscript_of(words: i32) -> Manuscript {
        Manuscript {
            title: "T".into(),
            subtitle: None,
            byline: "A".into(),
            surname: "A".into(),
            short_title: "T".into(),
            contact: Contact::default(),
            copyright_line: None,
            dedication: None,
            epigraph: None,
            epigraph_source: None,
            word_count: words,
            chapters: vec![],
        }
    }

    #[test]
    fn word_counts_round_the_way_shunn_asks() {
        assert_eq!(manuscript_of(412).rounded_word_count(), 400);
        assert_eq!(manuscript_of(1_449).rounded_word_count(), 1_400);
        assert_eq!(manuscript_of(7_240).rounded_word_count(), 7_000);
        assert_eq!(manuscript_of(7_260).rounded_word_count(), 7_500);
        assert_eq!(manuscript_of(78_412).rounded_word_count(), 78_000);
        assert_eq!(manuscript_of(0).rounded_word_count(), 0);
    }
}
