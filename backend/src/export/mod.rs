//! Turning a frozen version into something you can send someone.
//!
//! One assembled `Manuscript` feeds every writer, so the formats cannot disagree
//! about what the book says — only about how it looks. The TipTap JSON is
//! flattened into a small block model here, once, rather than in three places.

pub mod assemble;
pub mod docx;
pub mod epub;
pub mod html;
pub mod markdown;
pub mod options;
pub mod pdf;
pub mod pipeline;
pub mod project;
pub mod text;
pub mod web;

use serde_json::Value;

/// A run of text with the marks that survive export.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Span {
    pub text: String,
    pub italic: bool,
    pub bold: bool,
    pub underline: bool,
    /// A reference mark to the note with this number, in place of text. Set
    /// only by the notes stage; a span is either text or a mark.
    pub note: Option<u32>,
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

#[derive(Debug, Clone, Default)]
pub struct SceneContent {
    pub id: String,
    /// Carried but not printed: scene titles are the writer's scaffolding, not
    /// the reader's. Kept because an outline export would want them.
    #[allow(dead_code)]
    pub title: String,
    pub blocks: Vec<Block>,
}

/// Where a chapter sits in the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    #[default]
    Chapter,
    Front,
    Back,
}

impl Role {
    pub fn parse(value: &str) -> Self {
        match value {
            "front" => Self::Front,
            "back" => Self::Back,
            _ => Self::Chapter,
        }
    }
}

/// One comment, turned into a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub number: u32,
    pub text: String,
    /// The passage it was made on, when the note is to quote it.
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ChapterContent {
    pub id: String,
    pub title: String,
    pub role: Role,
    pub scenes: Vec<SceneContent>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone)]
pub struct Cover {
    pub bytes: Vec<u8>,
    /// `image/jpeg` or `image/png`.
    pub mime: String,
}

impl Cover {
    pub fn extension(&self) -> &'static str {
        if self.mime == "image/png" {
            "png"
        } else {
            "jpg"
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contributor {
    pub name: String,
    /// `editor`, `translator`, `illustrator` or `co-author`.
    pub role: String,
}

/// Where the notes go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteLayout {
    #[default]
    None,
    Footnotes,
    EndnotesChapter,
    EndnotesBook,
}

#[derive(Debug, Clone, Default)]
pub struct Contact {
    pub legal_name: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Clone, Default)]
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
    /// BCP 47 tag.
    pub language: String,
    pub rtl: bool,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published: Option<String>,
    pub identifier: Option<String>,
    pub keywords: Vec<String>,
    pub contributors: Vec<Contributor>,
    pub cover: Option<Cover>,
    /// Leave out the title page and the dedication, epigraph and copyright line.
    pub omit_front_matter: bool,
    /// Print a contents list (a navigation document is always written in EPUB).
    pub toc: bool,
    pub note_layout: NoteLayout,
    /// Prefix each note with "Comment:".
    pub note_label: bool,
    /// For a frozen version: when it was taken, and an id to derive the
    /// package id from, so exporting it again gives the same bytes. A draft
    /// has neither and is stamped with the moment of export.
    pub frozen_at: Option<String>,
    pub frozen_id: Option<String>,
}

/// Which set of conventions to lay the text out under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    /// Shunn standard manuscript format: Courier 12pt, double spaced, indented
    /// paragraphs, `#` scene breaks, contact block, running header. What a
    /// submission to a magazine or an agent is expected to look like.
    Manuscript,
    /// Something closer to a finished book: proportional type, single spaced,
    /// a glyph for scene breaks, chapter title pages.
    Book,
}

impl Note {
    /// The note as one line of plain text: label, the passage it was made on if
    /// asked for, then what the writer wrote.
    pub fn plain(&self, label: bool) -> String {
        let mut out = String::new();
        if label {
            out.push_str("Comment: ");
        }
        if let Some(quote) = &self.quote {
            out.push_str(&format!("“{}” — ", quote.trim()));
        }
        out.push_str(&self.text);
        out
    }
}

impl Manuscript {
    /// Every note in the book, in order, for endnotes that gather at the end.
    pub fn all_notes(&self) -> Vec<&Note> {
        self.chapters.iter().flat_map(|c| c.notes.iter()).collect()
    }

    /// Whether the writer has filled in anything beyond title and byline. A
    /// file made without any of it comes out as it always did.
    pub fn has_details(&self) -> bool {
        self.description.is_some()
            || self.publisher.is_some()
            || self.published.is_some()
            || self.identifier.is_some()
            || !self.keywords.is_empty()
            || !self.contributors.is_empty()
            || self.cover.is_some()
            || (!self.language.is_empty() && self.language != "en")
    }

    /// The language tag, `en` when none was set.
    pub fn lang(&self) -> &str {
        if self.language.is_empty() {
            "en"
        } else {
            &self.language
        }
    }

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

/// ProseMirror's size for a node: a character is one unit (UTF-16, as in the
/// editor), a leaf is one, and anything with content is its content plus its
/// open and close tokens.
fn node_size(node: &Value) -> u32 {
    match node.get("type").and_then(Value::as_str) {
        Some("text") => node
            .get("text")
            .and_then(Value::as_str)
            .map_or(0, |t| t.encode_utf16().count() as u32),
        Some("hardBreak" | "horizontalRule" | "image") => 1,
        _ => {
            2 + node
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(node_size)
                .sum::<u32>()
        }
    }
}

/// Flattens one scene's TipTap document into blocks.
pub fn blocks_from_doc(doc: &Value) -> Vec<Block> {
    positioned_blocks(doc)
        .into_iter()
        .map(|(block, _)| block)
        .collect()
}

/// The same blocks, each with the editor position its first character sits at.
/// A comment is stored as a pair of those positions, so this is what lets an
/// export put a note back beside its passage.
pub fn positioned_blocks(doc: &Value) -> Vec<(Block, u32)> {
    let mut blocks = Vec::new();
    let mut pos: u32 = 0;
    for node in doc
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let start = pos + 1;
        pos += node_size(node);
        match node.get("type").and_then(Value::as_str) {
            Some("paragraph") => {
                let spans = spans_from(node);
                // Empty paragraphs are the writer pressing return, not content.
                // Carrying them through would double-space an already
                // double-spaced manuscript.
                if !spans.is_empty() {
                    blocks.push((Block::Paragraph(spans), start));
                }
            }
            Some("heading") => {
                let level = node
                    .get("attrs")
                    .and_then(|a| a.get("level"))
                    .and_then(Value::as_u64)
                    .unwrap_or(2) as u8;
                blocks.push((
                    Block::Heading {
                        level,
                        spans: spans_from(node),
                    },
                    start,
                ));
            }
            Some("blockquote") => blocks.push((Block::Quote(spans_from(node)), start + 1)),
            Some("horizontalRule") => blocks.push((Block::SceneBreak, start)),
            Some("bulletList" | "orderedList") => {
                // Lists are rare in fiction and have no manuscript convention;
                // each item becomes its own paragraph rather than being lost.
                let mut item_pos = start;
                for item in node
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let spans = spans_from(item);
                    if !spans.is_empty() {
                        blocks.push((Block::Paragraph(spans), item_pos + 2));
                    }
                    item_pos += node_size(item);
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
            ..Default::default()
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

/// Default options must reproduce the output as it was before options existed.
/// The fixtures were written by the writers before they took options; set
/// `UPDATE_GOLDEN=1` to rewrite them deliberately.
#[cfg(test)]
mod golden {
    use super::*;

    pub(super) fn fixture() -> Manuscript {
        let plain = |t: &str| Span {
            text: t.into(),
            ..Default::default()
        };
        let italic = |t: &str| Span {
            text: t.into(),
            italic: true,
            ..Default::default()
        };
        Manuscript {
            title: "The Long Road".into(),
            subtitle: Some("A Novel".into()),
            byline: "Ada Quill".into(),
            surname: "Quill".into(),
            short_title: "ROAD".into(),
            contact: Contact {
                legal_name: Some("Ada Q. Quill".into()),
                address: Some("1 Lane\nTown".into()),
                phone: Some("555-0100".into()),
                email: Some("ada@example.com".into()),
            },
            copyright_line: Some("© 2026 Ada Quill".into()),
            dedication: Some("For Sam".into()),
            epigraph: Some("Whatever comes.".into()),
            epigraph_source: Some("Anon".into()),
            word_count: 1_234,
            chapters: vec![
                ChapterContent {
                    title: "Chapter 1".into(),
                    scenes: vec![
                        SceneContent {
                            title: "Scene 1".into(),
                            blocks: vec![
                                Block::Paragraph(vec![plain("She said "), italic("nothing"), plain(" at all & left.")]),
                                Block::Heading { level: 3, spans: vec![plain("Aside")] },
                                Block::Quote(vec![plain("A quoted line.")]),
                            ],
                            ..Default::default()
                        },
                        SceneContent {
                            title: "Scene 2".into(),
                            blocks: vec![Block::Paragraph(vec![plain("Later.\nMuch later.")]), Block::SceneBreak, Block::Paragraph(vec![plain("End.")])],
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                ChapterContent {
                    title: "Chapter 2".into(),
                    scenes: vec![SceneContent {
                        title: "Scene 1".into(),
                        blocks: vec![Block::Paragraph(vec![plain("Another day.")])],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            language: "en".into(),
            ..Default::default()
        }
    }

    /// Fields that differ on every run (a random id, a clock) are blanked
    /// before comparing.
    pub(super) fn normalise_epub(bytes: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = Vec::new();
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).unwrap();
            let mut body = String::new();
            let name = file.name().to_string();
            if file.read_to_string(&mut body).is_err() {
                continue;
            }
            let body = body
                .lines()
                .map(|l| {
                    if l.contains("dc:identifier") || l.contains("dcterms:modified") {
                        "<!-- varies -->"
                    } else {
                        l
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            out.extend_from_slice(format!("== {name}\n{body}\n").as_bytes());
        }
        out
    }

    pub(super) fn check(name: &str, actual: &[u8]) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read(&path)
            .unwrap_or_else(|_| panic!("missing golden file {name}; run with UPDATE_GOLDEN=1"));
        assert!(
            expected == actual,
            "{name} no longer matches its golden file; if the change is deliberate, rerun with UPDATE_GOLDEN=1"
        );
    }

    fn opt(style: Style) -> options::ExportOptions {
        options::ExportOptions::shorthand(options::Format::Docx, style)
    }

    #[test]
    fn default_options_reproduce_the_original_output() {
        let m = fixture();
        check("manuscript.docx", &docx::write(&m, &opt(Style::Manuscript)).unwrap());
        check("book.docx", &docx::write(&m, &opt(Style::Book)).unwrap());
        check("manuscript.txt", text::write(&m, &opt(Style::Manuscript)).as_bytes());
        check("book.txt", text::write(&m, &opt(Style::Book)).as_bytes());
        check("book.html", html::write(&m, &opt(Style::Book)).as_bytes());
        check("book.pdf", &pdf::write(&m, &opt(Style::Manuscript)));
        check("book.epub.txt", &normalise_epub(&epub::write(&m, &opt(Style::Book)).unwrap()));
    }
}
