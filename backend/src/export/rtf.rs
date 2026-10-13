//! RTF in, an editor document out — enough of RTF for the prose a writer keeps
//! in Scrivener: paragraphs, line breaks, bold, italic, underline and strike.
//!
//! Everything else (fonts, colours, styles, pictures, footnotes, page setup) is
//! read past rather than carried over. Text is never dropped for want of
//! understanding a control word: an unknown word is ignored and the text
//! around it kept, and only groups RTF marks as destinations (`{\*\...}`) or
//! that are known to hold no prose are skipped whole.
use serde_json::{json, Value};

use crate::domain::doc;

/// Groups whose contents are never part of the text.
const SKIPPED: &[&str] = &[
    "fonttbl",
    "colortbl",
    "expandedcolortbl",
    "stylesheet",
    "info",
    "listtable",
    "listoverridetable",
    "revtbl",
    "pict",
    "object",
    "fldinst",
    "header",
    "headerl",
    "headerr",
    "headerf",
    "footer",
    "footerl",
    "footerr",
    "footerf",
    "footnote",
    "NeXTGraphic",
    "themedata",
    "colorschememapping",
    "latentstyles",
    "datastore",
    "xmlnstbl",
    "generator",
];

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Marks {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

#[derive(Clone, Copy)]
struct Group {
    marks: Marks,
    skip: bool,
    /// How many fallback characters follow each `\uN`.
    uc: u32,
}

enum Inline {
    Text(String, Marks),
    Break,
}

struct Builder {
    paragraphs: Vec<Vec<Inline>>,
    current: Vec<Inline>,
}

impl Builder {
    fn push(&mut self, ch: char, marks: Marks) {
        if let Some(Inline::Text(text, m)) = self.current.last_mut() {
            if *m == marks {
                text.push(ch);
                return;
            }
        }
        self.current.push(Inline::Text(ch.to_string(), marks));
    }

    fn end_paragraph(&mut self) {
        self.paragraphs.push(std::mem::take(&mut self.current));
    }
}

/// The document an RTF file holds. Bytes that are not RTF are read as plain
/// text, so a `.txt` that found its way into a project still comes through.
pub fn parse(bytes: &[u8]) -> Value {
    if !bytes.starts_with(b"{\\rtf") {
        let text = String::from_utf8_lossy(bytes);
        return plain(&text);
    }
    let mut out = Builder {
        paragraphs: Vec::new(),
        current: Vec::new(),
    };
    let mut stack: Vec<Group> = Vec::new();
    let mut group = Group {
        marks: Marks::default(),
        skip: false,
        uc: 1,
    };
    // Fallback characters still to drop after a `\uN`.
    let mut pending_skip = 0u32;
    let mut high_surrogate: Option<u16> = None;
    // The first word in a group decides whether it is a destination.
    let mut group_start = false;
    let mut star = false;
    // A Cocoa picture attachment leaves a stray `¬` right after its (skipped)
    // group.
    let mut after_graphic = false;

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'{' => {
                stack.push(group);
                group_start = true;
                star = false;
                after_graphic = false;
                i += 1;
            }
            b'}' => {
                let skipped = group.skip;
                group = stack.pop().unwrap_or(group);
                after_graphic = skipped && !group.skip;
                group_start = false;
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&next) = bytes.get(i) else { break };
                if next.is_ascii_alphabetic() {
                    let start = i;
                    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
                    let num_start = i;
                    if i < bytes.len() && (bytes[i] == b'-' || bytes[i].is_ascii_digit()) {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let param: Option<i32> = std::str::from_utf8(&bytes[num_start..i])
                        .ok()
                        .and_then(|s| s.parse().ok());
                    // One space ends a control word and is part of it.
                    if i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }

                    let first = group_start;
                    group_start = false;
                    if first && (star || SKIPPED.contains(&word)) {
                        group.skip = true;
                    }
                    star = false;
                    if group.skip {
                        continue;
                    }
                    let on = param != Some(0);
                    match word {
                        "par" | "sect" | "page" => out.end_paragraph(),
                        "line" => out.current.push(Inline::Break),
                        "tab" => out.push('\t', group.marks),
                        "b" => group.marks.bold = on,
                        "i" => group.marks.italic = on,
                        "ul" | "uld" | "uldb" | "ulw" | "ulth" => group.marks.underline = on,
                        "ulnone" => group.marks.underline = false,
                        "strike" | "striked" => group.marks.strike = on,
                        "plain" => group.marks = Marks::default(),
                        "uc" => group.uc = param.unwrap_or(1).max(0) as u32,
                        "u" => {
                            let unit = param.unwrap_or(0) as i64;
                            let unit = if unit < 0 { unit + 65_536 } else { unit } as u16;
                            if (0xD800..0xDC00).contains(&unit) {
                                high_surrogate = Some(unit);
                            } else if let Some(high) = high_surrogate.take() {
                                if let Some(ch) = char::decode_utf16([high, unit]).next() {
                                    out.push(ch.unwrap_or('\u{FFFD}'), group.marks);
                                }
                            } else if let Some(ch) = char::from_u32(unit as u32) {
                                out.push(ch, group.marks);
                            }
                            pending_skip = group.uc;
                        }
                        "emdash" => out.push('—', group.marks),
                        "endash" => out.push('–', group.marks),
                        "lquote" => out.push('‘', group.marks),
                        "rquote" => out.push('’', group.marks),
                        "ldblquote" => out.push('“', group.marks),
                        "rdblquote" => out.push('”', group.marks),
                        "bullet" => out.push('•', group.marks),
                        _ => {}
                    }
                    continue;
                }
                i += 1;
                match next {
                    b'*' => {
                        star = true;
                        continue;
                    }
                    b'\'' => {
                        let hex = bytes
                            .get(i..i + 2)
                            .and_then(|h| std::str::from_utf8(h).ok());
                        i += 2;
                        group_start = false;
                        if group.skip {
                            continue;
                        }
                        if pending_skip > 0 {
                            pending_skip -= 1;
                            continue;
                        }
                        if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                            let ch = cp1252(byte);
                            if !(after_graphic && ch == '¬') {
                                out.push(ch, group.marks);
                            }
                        }
                        after_graphic = false;
                    }
                    b'\n' | b'\r' if !group.skip => out.end_paragraph(),
                    b'~' if !group.skip => out.push('\u{a0}', group.marks),
                    b'_' if !group.skip => out.push('\u{2011}', group.marks),
                    b'{' | b'}' | b'\\' if !group.skip => {
                        if pending_skip > 0 {
                            pending_skip -= 1;
                        } else {
                            out.push(next as char, group.marks);
                        }
                    }
                    _ => {}
                }
                group_start = false;
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                // Literal text. RTF is 7-bit by the book, but Cocoa writes some
                // characters raw, in the document's code page.
                group_start = false;
                star = false;
                i += 1;
                if group.skip {
                    continue;
                }
                if pending_skip > 0 {
                    pending_skip -= 1;
                    continue;
                }
                let ch = cp1252(b);
                if after_graphic && ch == '¬' {
                    after_graphic = false;
                    continue;
                }
                after_graphic = false;
                out.push(ch, group.marks);
            }
        }
    }
    if !out.current.is_empty() {
        out.end_paragraph();
    }
    build(out.paragraphs)
}

/// A plain-text file as a document: a paragraph per line.
pub fn plain(text: &str) -> Value {
    let paragraphs = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(|line| vec![Inline::Text(line.to_string(), Marks::default())])
        .collect();
    build(paragraphs)
}

fn build(paragraphs: Vec<Vec<Inline>>) -> Value {
    let mut blocks = Vec::new();
    for paragraph in paragraphs {
        let mut content: Vec<Value> = Vec::new();
        let mut leading = true;
        for inline in paragraph {
            match inline {
                Inline::Break => {
                    if !content.is_empty() {
                        content.push(json!({ "type": "hardBreak" }));
                    }
                }
                Inline::Text(text, marks) => {
                    let text = strip_style_tags(&text);
                    // Indents were the first-line indent made by hand.
                    let text = if leading {
                        text.trim_start_matches(['\t', ' ']).to_string()
                    } else {
                        text
                    };
                    if text.is_empty() {
                        continue;
                    }
                    leading = false;
                    let mut node = json!({ "type": "text", "text": text });
                    let mut list = Vec::new();
                    if marks.bold {
                        list.push(json!({ "type": "bold" }));
                    }
                    if marks.italic {
                        list.push(json!({ "type": "italic" }));
                    }
                    if marks.underline {
                        list.push(json!({ "type": "underline" }));
                    }
                    if marks.strike {
                        list.push(json!({ "type": "strike" }));
                    }
                    if !list.is_empty() {
                        node["marks"] = Value::Array(list);
                    }
                    content.push(node);
                }
            }
        }
        while content.last().is_some_and(|n| n["type"] == "hardBreak") {
            content.pop();
        }
        let text: String = content
            .iter()
            .filter_map(|n| n["text"].as_str())
            .collect::<String>();
        let text = text.trim();
        if text.is_empty() {
            // Blank lines were spacing, which the editor gives every paragraph.
            continue;
        }
        if matches!(text, "#" | "*" | "***" | "* * *" | "~" | "§") {
            blocks.push(json!({ "type": "horizontalRule" }));
            continue;
        }
        blocks.push(json!({ "type": "paragraph", "content": content }));
    }
    if blocks.is_empty() {
        return doc::empty_doc();
    }
    json!({ "type": "doc", "content": blocks })
}

/// Scrivener 3 marks its named styles in the text itself: `<$Scr_Ps::0>`,
/// `<!$Scr_Cs::1>` and the like. They are formatting, not words.
fn strip_style_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let tag = tail
            .strip_prefix("<$Scr_")
            .or_else(|| tail.strip_prefix("<!$Scr_"));
        match tag.and_then(|t| t.find('>').map(|end| (t, end))) {
            Some((t, end)) if t[..end].contains("::") && !t[..end].contains('<') => {
                rest = &t[end + 1..];
            }
            _ => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Windows-1252, which Scrivener writes as `\ansicpg1252`; it is Latin-1 but
/// for the printable characters it puts at 0x80–0x9F.
fn cp1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[(byte - 0x80) as usize],
        _ => byte as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Value) -> Vec<String> {
        doc["content"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                b["content"]
                    .as_array()
                    .map(|c| c.iter().filter_map(|n| n["text"].as_str()).collect())
                    .unwrap_or_else(|| format!("<{}>", b["type"].as_str().unwrap()))
            })
            .collect()
    }

    /// What Scrivener 3 on a Mac writes for a short scene.
    const SCRIVENER: &str = r#"{\rtf1\ansi\ansicpg1252\cocoartf2709
\cocoatextscaling0\cocoaplatform0{\fonttbl\f0\fnil\fcharset0 Palatino-Roman;\f1\fnil\fcharset0 Palatino-Italic;}
{\colortbl;\red255\green255\blue255;}
{\*\expandedcolortbl;;}
\pard\tx360\fi360\sl264\slmult1\pardirnatural\partightenfactor0

\f0\fs26 \cf0 <$Scr_Ps::0>\tab The door was open. She had left it \i that\i0  way.\
\
Mara\'92s coat hung where it always had \'97 on the hook.\
#\
A new \b day\b0 , \u8220 ?quoted\u8221 ?.<!$Scr_Ps::0>}"#;

    #[test]
    fn a_scrivener_scene_reads_as_paragraphs() {
        let doc = parse(SCRIVENER.as_bytes());
        assert_eq!(
            texts(&doc),
            [
                "The door was open. She had left it that way.",
                "Mara’s coat hung where it always had — on the hook.",
                "<horizontalRule>",
                "A new day, “quoted”.",
            ]
        );
        let first = &doc["content"][0]["content"];
        assert_eq!(first[1]["text"], "that");
        assert_eq!(first[1]["marks"][0]["type"], "italic");
        assert_eq!(doc["content"][3]["content"][1]["marks"][0]["type"], "bold");
    }

    #[test]
    fn pictures_footnotes_and_field_codes_leave_only_the_text() {
        let rtf = br#"{\rtf1\ansi Before {\field{\*\fldinst{HYPERLINK "scrivcmt://X"}}{\fldrslt linked}} after{\footnote gone}.\
{{\NeXTGraphic image.png \width100 \height100 \noorient}\'ac}Picture above.\
{\*\unknowndest nothing}Kept {\pict\pngblip 89504e47} too.}"#;
        assert_eq!(
            texts(&parse(rtf)),
            ["Before linked after.", "Picture above.", "Kept  too."]
        );
    }

    #[test]
    fn unicode_escapes_skip_their_fallbacks() {
        let rtf = br#"{\rtf1\ansi\uc2 caf\u233 xx {\uc0 na\u239 ve} {\uc1 \u-10179 ?\u-8704 ?}}"#;
        assert_eq!(texts(&parse(rtf)), ["café naïve 😀"]);
    }

    #[test]
    fn an_empty_document_is_still_a_document() {
        let doc = parse(br"{\rtf1\ansi{\fonttbl\f0 Times;}\par\par}");
        assert_eq!(doc, doc::empty_doc());
        assert_eq!(
            texts(&parse(b"Plain text\n\nAgain")),
            ["Plain text", "Again"]
        );
    }

    #[test]
    fn only_scrivener_style_tags_are_stripped() {
        assert_eq!(
            strip_style_tags("<$Scr_Cs::1>a<!$Scr_Cs::1> < b <i>"),
            "a < b <i>"
        );
    }
}
