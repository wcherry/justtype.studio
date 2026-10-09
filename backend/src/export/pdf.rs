//! PDF output, in standard manuscript format.
//!
//! Written directly rather than through a layout library, which is only
//! reasonable because of the one decision that makes it so: Courier. It is a
//! base-14 font, so it never needs embedding, and it is monospaced, so every
//! glyph is exactly 0.6em and line breaking is arithmetic rather than metrics.
//! That is also precisely what standard manuscript format asks for, so the
//! constraint and the requirement are the same thing.
//!
//! Consequently this writer only produces the manuscript layout. A book-styled
//! PDF needs proportional type, which needs real font metrics — see the
//! roadmap. `Style::Book` is accepted and laid out as a manuscript rather than
//! refused, because a writer asking for a PDF wants a PDF.
use super::options::ExportOptions;
use super::{Block, Manuscript, Span};

const POINTS_PER_INCH: f32 = 72.0;
const PAGE_WIDTH: f32 = 8.5 * POINTS_PER_INCH;
const PAGE_HEIGHT: f32 = 11.0 * POINTS_PER_INCH;
const MARGIN: f32 = POINTS_PER_INCH;
const FONT_SIZE: f32 = 12.0;
/// Courier advances 0.6em per glyph, at every size.
const GLYPH_WIDTH: f32 = FONT_SIZE * 0.6;
/// Double spacing.
const LEADING: f32 = FONT_SIZE * 2.0;

fn columns() -> usize {
    ((PAGE_WIDTH - 2.0 * MARGIN) / GLYPH_WIDTH) as usize
}

fn lines_per_page() -> usize {
    ((PAGE_HEIGHT - 2.0 * MARGIN) / LEADING) as usize
}

/// A run of characters on one line, in one face.
#[derive(Debug, Clone)]
struct Run {
    text: String,
    italic: bool,
}

#[derive(Debug, Clone, Default)]
struct Line {
    runs: Vec<Run>,
    /// Columns of leading space — an indent, or centring.
    indent: usize,
    centred: bool,
}

impl Line {
    fn width(&self) -> usize {
        self.runs.iter().map(|r| r.text.chars().count()).sum()
    }

    fn plain(text: &str) -> Self {
        Line {
            runs: vec![Run {
                text: text.to_string(),
                italic: false,
            }],
            ..Default::default()
        }
    }

    fn centred(text: &str) -> Self {
        Line {
            centred: true,
            ..Line::plain(text)
        }
    }
}

/// Characters carrying their formatting, so wrapping can split anywhere without
/// losing which run a character belonged to.
fn flatten(spans: &[Span]) -> Vec<(char, bool)> {
    spans
        .iter()
        .flat_map(|span| {
            // A note reference is a bracketed number: Courier has no
            // superscript, and the notes it points to are listed at the end
            // of the chapter.
            let text = match span.note {
                Some(n) => format!("[{n}]"),
                None => span.text.clone(),
            };
            let italic = span.italic || span.bold;
            text.chars().map(move |ch| (ch, italic)).collect::<Vec<_>>()
        })
        .collect()
}

fn runs_from(chars: &[(char, bool)]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (ch, italic) in chars {
        match runs.last_mut() {
            Some(run) if run.italic == *italic => run.text.push(*ch),
            _ => runs.push(Run {
                text: ch.to_string(),
                italic: *italic,
            }),
        }
    }
    runs
}

/// Greedy wrap at word boundaries. A word longer than the measure is broken
/// rather than allowed to run into the margin.
fn wrap(spans: &[Span], first_indent: usize, width: usize) -> Vec<Line> {
    let chars = flatten(spans);
    let mut lines: Vec<Line> = Vec::new();
    let mut current: Vec<(char, bool)> = Vec::new();
    let mut indent = first_indent;

    let flush = |current: &mut Vec<(char, bool)>, indent: usize, lines: &mut Vec<Line>| {
        while current.last().is_some_and(|(c, _)| c.is_whitespace()) {
            current.pop();
        }
        lines.push(Line {
            runs: runs_from(current),
            indent,
            centred: false,
        });
        current.clear();
    };

    for (ch, italic) in chars {
        if ch == '\n' {
            flush(&mut current, indent, &mut lines);
            indent = 0;
            continue;
        }
        current.push((ch, italic));
        if current.len() + indent <= width {
            continue;
        }
        // Over the measure: step back to the last space, and carry the
        // unfinished word to the next line.
        match current.iter().rposition(|(c, _)| c.is_whitespace()) {
            Some(at) => {
                let tail: Vec<(char, bool)> = current.split_off(at + 1);
                flush(&mut current, indent, &mut lines);
                current = tail;
            }
            None => {
                // No word boundary to break at. The character that pushed it
                // over belongs to the next line, not this one.
                let overflow = current.pop().expect("pushed at least one char");
                flush(&mut current, indent, &mut lines);
                current.push(overflow);
            }
        }
        indent = 0;
    }
    if !current.is_empty() {
        flush(&mut current, indent, &mut lines);
    }
    lines
}

fn lay_out(manuscript: &Manuscript) -> Vec<Vec<Line>> {
    let width = columns();
    let mut lines: Vec<Line> = Vec::new();

    // Page one: contact block, word count, then the title a third of the way
    // down — the shape an editor's eye is used to skimming.
    let contact = &manuscript.contact;
    for value in if manuscript.omit_front_matter {
        Vec::new()
    } else {
        vec![
        contact
            .legal_name
            .clone()
            .or_else(|| Some(manuscript.byline.clone())),
        contact.address.clone(),
        contact.phone.clone(),
        contact.email.clone(),
    ]
    }
    .into_iter()
    .flatten()
    {
        for part in value.split('\n') {
            lines.push(Line::plain(part));
        }
    }

    if !manuscript.omit_front_matter {
        let count = format!("about {} words", manuscript.rounded_word_count());
        let pad = width.saturating_sub(count.chars().count());
        lines.push(Line::plain(&format!("{:pad$}{count}", "", pad = pad)));

        while lines.len() < lines_per_page() / 3 {
            lines.push(Line::default());
        }

        lines.push(Line::centred(&manuscript.title));
        if let Some(subtitle) = &manuscript.subtitle {
            lines.push(Line::centred(subtitle));
        }
        lines.push(Line::default());
        lines.push(Line::centred(&format!("by {}", manuscript.byline)));
        lines.push(Line::default());
        lines.push(Line::default());
    }

    let mut pages: Vec<Vec<Line>> = Vec::new();
    let push_page = |lines: &mut Vec<Line>, pages: &mut Vec<Vec<Line>>| {
        while lines.len() > lines_per_page() {
            let rest = lines.split_off(lines_per_page());
            pages.push(std::mem::replace(lines, rest));
        }
        pages.push(std::mem::take(lines));
    };

    // A contents page: titles only, since page numbers would need a second
    // pass over the layout.
    if manuscript.toc {
        push_page(&mut lines, &mut pages);
        lines.push(Line::centred("Contents"));
        lines.push(Line::default());
        for chapter in &manuscript.chapters {
            lines.push(Line::plain(&format!("     {}", chapter.title)));
        }
    }

    let notes_of = |notes: &[&super::Note], lines: &mut Vec<Line>| {
        if notes.is_empty() {
            return;
        }
        lines.push(Line::default());
        lines.push(Line::centred("Notes"));
        lines.push(Line::default());
        for note in notes {
            let text = format!("[{}] {}", note.number, note.plain(manuscript.note_label));
            lines.extend(wrap(
                &[Span {
                    text,
                    ..Default::default()
                }],
                0,
                width,
            ));
        }
    };

    for chapter in &manuscript.chapters {
        // A chapter begins on a fresh page, a third of the way down.
        push_page(&mut lines, &mut pages);
        for _ in 0..lines_per_page() / 3 {
            lines.push(Line::default());
        }
        lines.push(Line::centred(&chapter.title));
        lines.push(Line::default());

        for (index, scene) in chapter.scenes.iter().enumerate() {
            if index > 0 {
                lines.push(Line::centred("#"));
                lines.push(Line::default());
            }
            for block in &scene.blocks {
                match block {
                    // Half an inch of indent, in columns.
                    Block::Paragraph(spans) => lines.extend(wrap(spans, 5, width)),
                    Block::Quote(spans) => lines.extend(wrap(spans, 5, width - 5)),
                    Block::Heading { spans, .. } => {
                        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                        lines.push(Line::default());
                        lines.push(Line::centred(&text));
                        lines.push(Line::default());
                    }
                    Block::SceneBreak => {
                        lines.push(Line::centred("#"));
                        lines.push(Line::default());
                    }
                }
            }
        }
        if manuscript.note_layout != super::NoteLayout::EndnotesBook {
            let notes: Vec<&super::Note> = chapter.notes.iter().collect();
            notes_of(&notes, &mut lines);
        }
    }
    if manuscript.note_layout == super::NoteLayout::EndnotesBook {
        push_page(&mut lines, &mut pages);
        let notes = manuscript.all_notes();
        notes_of(&notes, &mut lines);
    }
    push_page(&mut lines, &mut pages);
    pages.retain(|page| !page.is_empty());
    pages
}

/// WinAnsi byte for a character, which is the encoding the base-14 fonts are
/// declared with. The punctuation the editor produces — smart quotes, dashes,
/// ellipses — lives above 0x7F and would otherwise come out as mojibake.
fn winansi(ch: char) -> u8 {
    match ch {
        '\u{2018}' => 0x91,
        '\u{2019}' => 0x92,
        '\u{201C}' => 0x93,
        '\u{201D}' => 0x94,
        '\u{2013}' => 0x96,
        '\u{2014}' => 0x97,
        '\u{2026}' => 0x85,
        '\u{00A0}' => 0x20,
        c if (c as u32) < 256 => c as u8,
        // Anything else has no representation in this encoding. A question mark
        // is visibly wrong, which is better than a silently wrong glyph.
        _ => b'?',
    }
}

/// PDF string literal: parentheses and backslashes are structural.
fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for ch in text.chars() {
        let byte = winansi(ch);
        if matches!(byte, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(byte);
    }
    out.push(b')');
    out
}

fn content_stream(page: &[Line], header: Option<&str>) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"BT\n");

    if let Some(header) = header {
        let pad = columns().saturating_sub(header.chars().count());
        let x = MARGIN + pad as f32 * GLYPH_WIDTH;
        let y = PAGE_HEIGHT - MARGIN + LEADING / 2.0;
        out.extend_from_slice(format!("/F1 {FONT_SIZE} Tf\n1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
        out.extend_from_slice(&pdf_string(header));
        out.extend_from_slice(b" Tj\n");
    }

    for (index, line) in page.iter().enumerate() {
        if line.runs.is_empty() {
            continue;
        }
        let indent = if line.centred {
            columns().saturating_sub(line.width()) / 2
        } else {
            line.indent
        };
        let x = MARGIN + indent as f32 * GLYPH_WIDTH;
        let y = PAGE_HEIGHT - MARGIN - FONT_SIZE - index as f32 * LEADING;
        out.extend_from_slice(format!("1 0 0 1 {x:.2} {y:.2} Tm\n").as_bytes());
        for run in &line.runs {
            // Consecutive Tj continue from where the last one ended, so runs on
            // a line need no repositioning — only a font change.
            let face = if run.italic { "/F2" } else { "/F1" };
            out.extend_from_slice(format!("{face} {FONT_SIZE} Tf\n").as_bytes());
            out.extend_from_slice(&pdf_string(&run.text));
            out.extend_from_slice(b" Tj\n");
        }
    }

    out.extend_from_slice(b"ET\n");
    out
}

pub fn write(manuscript: &Manuscript, _options: &ExportOptions) -> Vec<u8> {
    let pages = lay_out(manuscript);
    let page_count = pages.len().max(1);

    // Object numbering: 1 catalog, 2 page tree, 3 and 4 the fonts, then a page
    // and a content stream for each sheet.
    let first_page_object = 5;
    let mut objects: Vec<Vec<u8>> = Vec::new();

    let kids: String = (0..page_count)
        .map(|i| format!("{} 0 R ", first_page_object + i * 2))
        .collect();

    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>").into_bytes());
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier /Encoding /WinAnsiEncoding >>".to_vec(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Courier-Oblique /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );

    for (index, page) in pages.iter().enumerate() {
        let content_object = first_page_object + index * 2 + 1;
        let header = (index > 0).then(|| {
            format!(
                "{} / {} / {}",
                manuscript.surname,
                manuscript.short_title.to_uppercase(),
                index + 1
            )
        });
        let stream = content_stream(page, header.as_deref());

        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH:.0} {PAGE_HEIGHT:.0}] \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_object} 0 R >>"
            )
            .into_bytes(),
        );

        let mut object = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        object.extend_from_slice(&stream);
        object.extend_from_slice(b"\nendstream");
        objects.push(object);
    }

    let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
    // A binary comment marks the file as binary for tools that sniff it.
    out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");

    let mut offsets: Vec<usize> = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }

    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str) -> Vec<Span> {
        vec![Span {
            text: text.into(),
            ..Default::default()
        }]
    }

    #[test]
    fn wrapping_respects_the_measure() {
        let long = "word ".repeat(40);
        let lines = wrap(&spans(&long), 5, 65);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(
                line.width() + line.indent <= 65,
                "line ran past the measure"
            );
        }
    }

    #[test]
    fn only_the_first_line_of_a_paragraph_is_indented() {
        let lines = wrap(&spans(&"word ".repeat(40)), 5, 65);
        assert_eq!(lines[0].indent, 5);
        assert!(lines[1..].iter().all(|l| l.indent == 0));
    }

    #[test]
    fn a_word_longer_than_the_measure_is_broken_rather_than_overflowing() {
        let lines = wrap(&spans(&"x".repeat(200)), 0, 65);
        assert!(lines.len() >= 4);
        assert!(lines.iter().all(|l| l.width() <= 65));
    }

    #[test]
    fn italics_survive_wrapping_as_separate_runs() {
        let mixed = vec![
            Span {
                text: "plain ".into(),
                ..Default::default()
            },
            Span {
                text: "emphasised".into(),
                italic: true,
                ..Default::default()
            },
        ];
        let lines = wrap(&mixed, 0, 65);
        let runs = &lines[0].runs;
        assert_eq!(runs.len(), 2);
        assert!(!runs[0].italic);
        assert!(runs[1].italic);
    }

    #[test]
    fn smart_punctuation_maps_into_winansi() {
        assert_eq!(winansi('\u{2019}'), 0x92); // ’
        assert_eq!(winansi('\u{2014}'), 0x97); // —
        assert_eq!(winansi('\u{2026}'), 0x85); // …
        assert_eq!(winansi('a'), b'a');
        assert_eq!(winansi('\u{4e2d}'), b'?'); // no representation
    }

    #[test]
    fn pdf_strings_escape_structural_characters() {
        assert_eq!(pdf_string("a(b)c\\d"), b"(a\\(b\\)c\\\\d)".to_vec());
    }
}

#[cfg(test)]
mod note_tests {
    use super::*;
    use crate::export::{ChapterContent, Note, NoteLayout, SceneContent};

    fn text_of(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).to_string()
    }

    fn book() -> Manuscript {
        Manuscript {
            title: "T".into(),
            byline: "A".into(),
            surname: "A".into(),
            short_title: "T".into(),
            chapters: vec![ChapterContent {
                title: "One".into(),
                scenes: vec![SceneContent {
                    blocks: vec![Block::Paragraph(vec![
                        Span { text: "Hello".into(), ..Default::default() },
                        Span { note: Some(1), ..Default::default() },
                    ])],
                    ..Default::default()
                }],
                notes: vec![Note { number: 1, text: "the note".into(), quote: None }],
                ..Default::default()
            }],
            note_layout: NoteLayout::EndnotesChapter,
            ..Default::default()
        }
    }

    #[test]
    fn notes_are_bracketed_marks_with_a_list_after_the_chapter() {
        let pdf = text_of(&write(&book(), &ExportOptions::default()));
        assert!(pdf.contains("Hello[1]"));
        assert!(pdf.contains("[1] the note"));
        assert!(pdf.contains("Notes"));
    }

    #[test]
    fn front_matter_can_be_left_out() {
        let mut m = book();
        m.omit_front_matter = true;
        let pdf = text_of(&write(&m, &ExportOptions::default()));
        assert!(!pdf.contains("by A"));
        assert!(pdf.contains("One"));
    }
}
