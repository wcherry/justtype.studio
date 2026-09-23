//! Applying a tracked change to a document.
//!
//! Until now the server recorded proposals and their resolutions and left the
//! editing to whoever was looking at the document. That works for one accept in
//! one browser and for nothing else: it cannot accept twenty at once, cannot
//! work from a phone, and leaves the meaning of "accepted" different on every
//! client. So application lives here, where the document does.
//!
//! Positions are ProseMirror positions — the same coordinates comment anchors
//! use, which the web editor reports directly and `DocNode.blocks()` computes
//! on iOS.

use serde_json::{json, Value};

use super::doc;

/// One text node, and where it sits in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TextRef {
    /// Inclusive start, in ProseMirror positions.
    from: usize,
    /// Exclusive end.
    to: usize,
    block: usize,
    child: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The range does not correspond to text in this document.
    OutOfRange,
    /// The range spans a paragraph boundary.
    ///
    /// Refused rather than attempted: joining blocks means deciding which one's
    /// type and attributes survive, and a suggestion that crosses a paragraph
    /// is nearly always a selection accident rather than an intention.
    CrossesBlocks,
    /// The document is not shaped the way the editor's schema says it is.
    Malformed(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::OutOfRange => write!(f, "that range is no longer part of this scene"),
            EditError::CrossesBlocks => write!(
                f,
                "that change spans more than one paragraph, which cannot be applied automatically"
            ),
            EditError::Malformed(why) => write!(f, "the document is malformed: {why}"),
        }
    }
}

/// Every text node in document order, with its position range.
fn text_refs(document: &Value) -> Vec<TextRef> {
    let mut refs = Vec::new();
    let mut pos = 0usize;

    for (block_index, block) in document
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let leaf = doc::is_leaf(block);
        let mut inner = if leaf { pos } else { pos + 1 };

        if !leaf {
            for (child_index, child) in block
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let size = doc::node_size(child);
                if child.get("type").and_then(Value::as_str) == Some("text") {
                    refs.push(TextRef {
                        from: inner,
                        to: inner + size,
                        block: block_index,
                        child: child_index,
                    });
                }
                inner += size;
            }
        }
        pos += doc::node_size(block);
    }
    refs
}

/// The text a range covers, for checking a proposal still points at what its
/// author meant.
pub fn text_in_range(document: &Value, from: usize, to: usize) -> Option<String> {
    if to < from {
        return None;
    }
    let mut out = String::new();
    for reference in text_refs(document) {
        if reference.to <= from || reference.from >= to {
            continue;
        }
        let text = node_text(document, &reference)?;
        let units: Vec<u16> = text.encode_utf16().collect();
        let start = from.saturating_sub(reference.from).min(units.len());
        let end = (to - reference.from).min(units.len());
        out.push_str(&String::from_utf16_lossy(&units[start..end]));
    }
    Some(out)
}

fn node_text(document: &Value, reference: &TextRef) -> Option<String> {
    document
        .get("content")?
        .get(reference.block)?
        .get("content")?
        .get(reference.child)?
        .get("text")?
        .as_str()
        .map(str::to_string)
}

/// Finds every place a quotation appears, as position ranges.
///
/// Used to re-anchor a proposal whose document has moved on. Matching is done
/// over the block's own text so a match can never straddle a paragraph.
pub fn find_quote(document: &Value, quote: &str) -> Vec<(usize, usize)> {
    if quote.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    let refs = text_refs(document);

    // Group by block, since a quotation cannot span one.
    let mut block_indices: Vec<usize> = refs.iter().map(|r| r.block).collect();
    block_indices.dedup();

    for block in block_indices {
        let in_block: Vec<&TextRef> = refs.iter().filter(|r| r.block == block).collect();
        let Some(first) = in_block.first() else {
            continue;
        };
        let mut text = String::new();
        for reference in &in_block {
            if let Some(chunk) = node_text(document, reference) {
                text.push_str(&chunk);
            }
        }

        // UTF-16 offsets, because that is what ProseMirror counts in.
        let haystack: Vec<u16> = text.encode_utf16().collect();
        let needle: Vec<u16> = quote.encode_utf16().collect();
        if needle.is_empty() || needle.len() > haystack.len() {
            continue;
        }
        for start in 0..=(haystack.len() - needle.len()) {
            if haystack[start..start + needle.len()] == needle[..] {
                found.push((first.from + start, first.from + start + needle.len()));
            }
        }
    }
    found
}

/// Replaces a range with text, keeping the marks of the text it replaces.
///
/// The replacement inherits the formatting at the start of the range, which is
/// what a reviewer correcting a word inside an italic phrase expects.
pub fn replace_range(
    document: &Value,
    from: usize,
    to: usize,
    replacement: &str,
) -> Result<Value, EditError> {
    if to < from {
        return Err(EditError::OutOfRange);
    }
    let refs = text_refs(document);

    // An insertion point may sit at the boundary of a text node; a range has to
    // overlap one.
    let touched: Vec<&TextRef> = refs
        .iter()
        .filter(|r| {
            if from == to {
                r.from <= from && from <= r.to
            } else {
                r.to > from && r.from < to
            }
        })
        .collect();

    let Some(first) = touched.first() else {
        return Err(EditError::OutOfRange);
    };
    if touched.iter().any(|r| r.block != first.block) {
        return Err(EditError::CrossesBlocks);
    }

    let block_index = first.block;
    let mut document = document.clone();
    let children = document
        .get_mut("content")
        .and_then(|c| c.get_mut(block_index))
        .and_then(|b| b.get_mut("content"))
        .and_then(Value::as_array_mut)
        .ok_or_else(|| EditError::Malformed("block has no content".into()))?;

    // Rebuilt rather than spliced in place: the range can cover parts of
    // several text nodes, and stitching the survivors together is clearer than
    // mutating each one.
    let mut rebuilt: Vec<Value> = Vec::new();
    let mut inserted = false;
    let marks = children
        .get(first.child)
        .and_then(|node| node.get("marks"))
        .cloned();

    for (index, child) in children.iter().enumerate() {
        let Some(reference) = touched.iter().find(|r| r.child == index) else {
            rebuilt.push(child.clone());
            continue;
        };
        let text = child.get("text").and_then(Value::as_str).unwrap_or("");
        let units: Vec<u16> = text.encode_utf16().collect();

        let head_end = from.saturating_sub(reference.from).min(units.len());
        let tail_start = to.saturating_sub(reference.from).min(units.len());

        let head = String::from_utf16_lossy(&units[..head_end]);
        if !head.is_empty() {
            rebuilt.push(with_text(child, &head));
        }
        if !inserted {
            if !replacement.is_empty() {
                let mut node = json!({ "type": "text", "text": replacement });
                if let Some(marks) = &marks {
                    node["marks"] = marks.clone();
                }
                rebuilt.push(node);
            }
            inserted = true;
        }
        let tail = String::from_utf16_lossy(&units[tail_start..]);
        if !tail.is_empty() {
            rebuilt.push(with_text(child, &tail));
        }
    }

    *children = rebuilt;
    // A paragraph emptied by a deletion keeps its `content` key absent rather
    // than an empty array, which is what the editor produces for a blank one.
    if children.is_empty() {
        if let Some(block) = document
            .get_mut("content")
            .and_then(|c| c.get_mut(block_index))
            .and_then(Value::as_object_mut)
        {
            block.remove("content");
        }
    }
    Ok(document)
}

fn with_text(node: &Value, text: &str) -> Value {
    let mut copy = node.clone();
    copy["text"] = Value::String(text.to_string());
    copy
}

/// Adds or removes a mark across a range.
pub fn set_mark(
    document: &Value,
    from: usize,
    to: usize,
    mark: &str,
    on: bool,
) -> Result<Value, EditError> {
    if to <= from {
        return Err(EditError::OutOfRange);
    }
    let text = text_in_range(document, from, to).ok_or(EditError::OutOfRange)?;
    // Split the range out into its own node, then mark that node.
    let mut document = replace_range(document, from, to, &text)?;

    let refs = text_refs(&document);
    let target = refs
        .iter()
        .find(|r| r.from == from && r.to == to)
        .ok_or(EditError::OutOfRange)?;

    let node = document
        .get_mut("content")
        .and_then(|c| c.get_mut(target.block))
        .and_then(|b| b.get_mut("content"))
        .and_then(|c| c.get_mut(target.child))
        .ok_or_else(|| EditError::Malformed("lost the node being marked".into()))?;

    let mut marks: Vec<Value> = node
        .get("marks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    marks.retain(|m| m.get("type").and_then(Value::as_str) != Some(mark));
    if on {
        marks.push(json!({ "type": mark }));
    }

    if let Some(object) = node.as_object_mut() {
        if marks.is_empty() {
            object.remove("marks");
        } else {
            object.insert("marks".into(), Value::Array(marks));
        }
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"The quick brown fox."}]},
            {"type":"paragraph","content":[{"type":"text","text":"Second paragraph here."}]}
        ]})
    }

    #[test]
    fn positions_match_prosemirror() {
        // First paragraph opens at 0, so its text runs 1..21.
        assert_eq!(text_in_range(&sample(), 1, 4).unwrap(), "The");
        assert_eq!(text_in_range(&sample(), 5, 10).unwrap(), "quick");
        // Second paragraph starts after the first closes at 21.
        assert_eq!(text_in_range(&sample(), 23, 29).unwrap(), "Second");
    }

    #[test]
    fn replacing_swaps_the_words() {
        let edited = replace_range(&sample(), 5, 10, "slow").unwrap();
        assert_eq!(
            doc::plain_text(&edited).lines().next().unwrap(),
            "The slow brown fox."
        );
    }

    #[test]
    fn deleting_is_replacing_with_nothing() {
        let edited = replace_range(&sample(), 4, 10, "").unwrap();
        assert_eq!(
            doc::plain_text(&edited).lines().next().unwrap(),
            "The brown fox."
        );
    }

    #[test]
    fn inserting_uses_an_empty_range() {
        let edited = replace_range(&sample(), 4, 4, " very").unwrap();
        assert_eq!(
            doc::plain_text(&edited).lines().next().unwrap(),
            "The very quick brown fox."
        );
    }

    #[test]
    fn a_replacement_inherits_the_formatting_it_lands_in() {
        let italic = json!({"type":"doc","content":[
            {"type":"paragraph","content":[
                {"type":"text","text":"She said "},
                {"type":"text","text":"nothing","marks":[{"type":"italic"}]}
            ]}
        ]});
        let edited = replace_range(&italic, 10, 17, "something").unwrap();
        let nodes = edited["content"][0]["content"].as_array().unwrap();
        let replaced = nodes.iter().find(|n| n["text"] == "something").unwrap();
        assert_eq!(replaced["marks"][0]["type"], "italic");
    }

    #[test]
    fn a_change_spanning_two_paragraphs_is_refused() {
        assert_eq!(
            replace_range(&sample(), 5, 26, "x"),
            Err(EditError::CrossesBlocks)
        );
    }

    #[test]
    fn a_range_past_the_end_is_refused() {
        assert_eq!(
            replace_range(&sample(), 900, 910, "x"),
            Err(EditError::OutOfRange)
        );
    }

    #[test]
    fn quotes_are_found_with_prosemirror_positions() {
        let found = find_quote(&sample(), "quick");
        assert_eq!(found, vec![(5, 10)]);
        assert_eq!(text_in_range(&sample(), 5, 10).unwrap(), "quick");
    }

    #[test]
    fn an_ambiguous_quote_reports_every_match() {
        let document = json!({"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"cat and cat"}]}
        ]});
        assert_eq!(find_quote(&document, "cat").len(), 2);
    }

    #[test]
    fn a_quote_cannot_match_across_a_paragraph_boundary() {
        assert!(find_quote(&sample(), "fox. Second").is_empty());
    }

    #[test]
    fn marks_can_be_added_and_taken_off_again() {
        let marked = set_mark(&sample(), 5, 10, "italic", true).unwrap();
        let nodes = marked["content"][0]["content"].as_array().unwrap();
        let target = nodes.iter().find(|n| n["text"] == "quick").unwrap();
        assert_eq!(target["marks"][0]["type"], "italic");

        let cleared = set_mark(&marked, 5, 10, "italic", false).unwrap();
        let nodes = cleared["content"][0]["content"].as_array().unwrap();
        let target = nodes.iter().find(|n| n["text"] == "quick").unwrap();
        assert!(target.get("marks").is_none());
        assert_eq!(doc::plain_text(&cleared), doc::plain_text(&sample()));
    }

    #[test]
    fn utf16_positions_survive_astral_characters() {
        // An emoji is two UTF-16 units, which is what ProseMirror counts.
        let document = json!({"type":"doc","content":[
            {"type":"paragraph","content":[{"type":"text","text":"a🙂b"}]}
        ]});
        assert_eq!(text_in_range(&document, 1, 2).unwrap(), "a");
        assert_eq!(text_in_range(&document, 4, 5).unwrap(), "b");
        let edited = replace_range(&document, 4, 5, "c").unwrap();
        assert_eq!(doc::plain_text(&edited), "a🙂c");
    }

    /// Deleting a paragraph's text is not deleting the paragraph — the blank
    /// one stays, the way it does in every other editor. Removing it is the
    /// writer's call, not a side effect of accepting a suggestion.
    #[test]
    fn emptying_a_paragraph_leaves_it_blank_rather_than_removing_it() {
        let edited = replace_range(&sample(), 1, 21, "").unwrap();
        assert!(doc::validate(&edited).is_ok());
        assert_eq!(doc::block_count(&edited), 2);
        assert_eq!(doc::plain_text(&edited), "\nSecond paragraph here.");
        assert!(edited["content"][0].get("content").is_none());
    }
}
