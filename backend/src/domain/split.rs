//! Splitting a piece of writing into scenes, for turning a Daily Prompts
//! answer into a project.
//!
//! The rules, which the web dialog and iOS both get from the server rather
//! than implement themselves:
//!
//! - A line that is only `--` (whitespace around it ignored) always separates
//!   scenes, and is dropped. A line inside a paragraph counts too, between
//!   hard breaks. `--` inside a sentence, an em dash, or `---` does not.
//! - A blank paragraph (double Enter) separates scenes only when the writer
//!   has turned that on. A run of them is one break.
//! - Scenes with nothing in them are dropped, so separators at the start or
//!   the end make no empty scenes. Text with no separators is one scene.
use serde_json::{json, Value};

/// Each scene as a document of its own, in order.
pub fn split_scenes(doc: &Value, blank_paragraph_splits: bool) -> Vec<Value> {
    let mut scenes: Vec<Vec<Value>> = vec![Vec::new()];
    let blocks = doc
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for block in blocks {
        for piece in separate(&block) {
            match piece {
                Piece::Separator => scenes.push(Vec::new()),
                Piece::Block(b) if blank_paragraph_splits && is_blank_paragraph(&b) => {
                    scenes.push(Vec::new())
                }
                Piece::Block(b) => scenes.last_mut().expect("never empty").push(b),
            }
        }
    }
    scenes
        .into_iter()
        .map(trim_blank_edges)
        .filter(|blocks| blocks.iter().any(|b| !is_blank_paragraph(b)))
        .map(|blocks| json!({ "type": "doc", "content": blocks }))
        .collect()
}

/// A scene's opening line, for the preview: its first text, shortened.
pub fn first_line(doc: &Value, max_chars: usize) -> String {
    let text = super::doc::plain_text(doc);
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    shorten(line, max_chars)
}

/// `text` cut at a word boundary to at most `max_chars`, with an ellipsis when
/// anything was cut.
pub fn shorten(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars).collect();
    // A clause boundary reads better than a word one ("…open…", not
    // "…open, and…"), when there is one far enough in.
    let half = cut.len() / 2;
    let at = cut
        .rfind([',', ';', ':', '.', '—'])
        .filter(|i| *i > half)
        .or_else(|| cut.rfind(char::is_whitespace).filter(|i| *i > half));
    let kept = match at {
        Some(i) => &cut[..i],
        None => &cut,
    };
    format!(
        "{}…",
        kept.trim_end_matches(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
    )
}

enum Piece {
    Block(Value),
    Separator,
}

fn is_separator_text(text: &str) -> bool {
    text.trim() == "--"
}

fn inline_text(nodes: &[Value]) -> String {
    nodes
        .iter()
        .filter_map(|n| n.get("text").and_then(Value::as_str))
        .collect()
}

fn is_hard_break(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("hardBreak")
}

fn is_blank_paragraph(block: &Value) -> bool {
    block.get("type").and_then(Value::as_str) == Some("paragraph")
        && block
            .get("content")
            .and_then(Value::as_array)
            .is_none_or(|nodes| {
                nodes.iter().all(|n| {
                    is_hard_break(n) || inline_text(std::slice::from_ref(n)).trim().is_empty()
                })
            })
}

/// A paragraph cut at any `--` line inside it. Other blocks pass through.
fn separate(block: &Value) -> Vec<Piece> {
    if block.get("type").and_then(Value::as_str) != Some("paragraph") {
        return vec![Piece::Block(block.clone())];
    }
    let nodes = block
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // Lines, as runs of inline nodes between hard breaks.
    let mut lines: Vec<Vec<Value>> = vec![Vec::new()];
    for node in nodes {
        if is_hard_break(&node) {
            lines.push(Vec::new());
        } else {
            lines.last_mut().expect("never empty").push(node);
        }
    }
    if !lines.iter().any(|l| is_separator_text(&inline_text(l))) {
        return vec![Piece::Block(block.clone())];
    }
    let mut out = Vec::new();
    let mut current: Vec<Vec<Value>> = Vec::new();
    let flush = |current: &mut Vec<Vec<Value>>, out: &mut Vec<Piece>| {
        if current.is_empty() {
            return;
        }
        let mut content = Vec::new();
        for (i, line) in current.drain(..).enumerate() {
            if i > 0 {
                content.push(json!({ "type": "hardBreak" }));
            }
            content.extend(line);
        }
        let mut paragraph = block.clone();
        paragraph["content"] = Value::Array(content);
        out.push(Piece::Block(paragraph));
    };
    for line in lines {
        if is_separator_text(&inline_text(&line)) {
            flush(&mut current, &mut out);
            out.push(Piece::Separator);
        } else {
            current.push(line);
        }
    }
    flush(&mut current, &mut out);
    out
}

fn trim_blank_edges(mut blocks: Vec<Value>) -> Vec<Value> {
    while blocks.first().is_some_and(is_blank_paragraph) {
        blocks.remove(0);
    }
    while blocks.last().is_some_and(is_blank_paragraph) {
        blocks.pop();
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(text: &str) -> Value {
        if text.is_empty() {
            json!({ "type": "paragraph" })
        } else {
            json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
        }
    }
    fn doc(blocks: Vec<Value>) -> Value {
        json!({ "type": "doc", "content": blocks })
    }
    fn texts(scenes: &[Value]) -> Vec<String> {
        scenes
            .iter()
            .map(|s| super::super::doc::plain_text(s).trim().replace('\n', " | "))
            .collect()
    }

    #[test]
    fn a_dash_dash_line_splits() {
        let d = doc(vec![
            p("One."),
            p("--"),
            p("Two."),
            p("  --  "),
            p("Three."),
        ]);
        assert_eq!(texts(&split_scenes(&d, false)), ["One.", "Two.", "Three."]);
    }

    #[test]
    fn dashes_in_a_sentence_an_em_dash_or_three_dashes_do_not() {
        let d = doc(vec![
            p("She paused -- then went on."),
            p("—"),
            p("---"),
            p("-- not alone"),
            p("Still one scene."),
        ]);
        assert_eq!(split_scenes(&d, false).len(), 1);
        assert_eq!(split_scenes(&d, true).len(), 1);
    }

    #[test]
    fn blank_paragraphs_split_only_with_the_setting_on() {
        let d = doc(vec![p("One."), p(""), p("Two.")]);
        assert_eq!(split_scenes(&d, false).len(), 1);
        assert_eq!(texts(&split_scenes(&d, true)), ["One.", "Two."]);
    }

    #[test]
    fn a_run_of_blank_paragraphs_is_one_break() {
        let d = doc(vec![p("One."), p(""), p(""), p("   "), p("Two.")]);
        assert_eq!(texts(&split_scenes(&d, true)), ["One.", "Two."]);
    }

    #[test]
    fn leading_and_trailing_separators_make_no_empty_scenes() {
        let d = doc(vec![
            p("--"),
            p(""),
            p("One."),
            p("--"),
            p("--"),
            p("Two."),
            p(""),
            p("--"),
        ]);
        assert_eq!(texts(&split_scenes(&d, false)), ["One.", "Two."]);
        assert_eq!(texts(&split_scenes(&d, true)), ["One.", "Two."]);
        assert!(split_scenes(&doc(vec![p("--"), p("")]), true).is_empty());
    }

    #[test]
    fn no_separators_is_one_scene_with_everything_kept() {
        let d = doc(vec![
            p("One."),
            json!({ "type": "blockquote", "content": [p("A letter.")] }),
            json!({ "type": "horizontalRule" }),
            p("Two."),
        ]);
        let scenes = split_scenes(&d, false);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0]["content"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn a_dash_dash_line_inside_a_paragraph_splits_it() {
        let d = doc(vec![json!({ "type": "paragraph", "content": [
            { "type": "text", "text": "Before." },
            { "type": "hardBreak" },
            { "type": "text", "text": "--" },
            { "type": "hardBreak" },
            { "type": "text", "text": "After.", "marks": [{ "type": "italic" }] },
        ]})]);
        let scenes = split_scenes(&d, false);
        assert_eq!(texts(&scenes), ["Before.", "After."]);
        // Marks survive the cut.
        assert_eq!(
            scenes[1]["content"][0]["content"][0]["marks"][0]["type"],
            "italic"
        );
    }

    #[test]
    fn previews_shorten_at_a_word() {
        let d = doc(vec![
            p(""),
            p("The keepers walked the causeway at low water, every one of them."),
        ]);
        assert_eq!(first_line(&d, 30), "The keepers walked the…");
        assert_eq!(shorten("Short.", 30), "Short.");
    }
}
