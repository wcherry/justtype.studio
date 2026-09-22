//! Helpers over the TipTap/ProseMirror JSON document shape. The editor owns the
//! schema; the server only needs to flatten a document to text (for word counts,
//! diffing and search) and to fingerprint it.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn empty_doc() -> Value {
    json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
}

pub fn doc_from_text(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .split("\n\n")
        .map(|para| {
            if para.trim().is_empty() {
                json!({ "type": "paragraph" })
            } else {
                json!({ "type": "paragraph", "content": [{ "type": "text", "text": para }] })
            }
        })
        .collect();
    json!({ "type": "doc", "content": paragraphs })
}

/// A document is valid if it is a `doc` node; anything else is a client bug and
/// we would rather reject it than store something the editor cannot open.
pub fn validate(doc: &Value) -> Result<(), String> {
    match doc.get("type").and_then(Value::as_str) {
        Some("doc") => Ok(()),
        Some(other) => Err(format!("expected a doc node, got `{other}`")),
        None => Err("document is missing a `type` field".into()),
    }
}

/// Block-level nodes become their own line so diffs land on paragraph
/// boundaries rather than smearing across the whole scene.
const BLOCK_TYPES: &[&str] = &[
    "paragraph",
    "heading",
    "blockquote",
    "listItem",
    "codeBlock",
    "horizontalRule",
];

pub fn plain_text(doc: &Value) -> String {
    let mut out = String::new();
    walk(doc, &mut out);
    out.trim_end().to_string()
}

fn walk(node: &Value, out: &mut String) {
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
    if node_type == "text" {
        if let Some(text) = node.get("text").and_then(Value::as_str) {
            out.push_str(text);
        }
        return;
    }
    if node_type == "hardBreak" {
        out.push('\n');
        return;
    }
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            walk(child, out);
        }
    }
    if BLOCK_TYPES.contains(&node_type) && !out.ends_with('\n') {
        out.push('\n');
    }
}

pub fn word_count(doc: &Value) -> i32 {
    plain_text(doc).split_whitespace().count() as i32
}

pub fn content_hash(doc: &Value) -> String {
    // serde_json serialises maps in insertion/sorted order consistently for
    // values parsed from the same bytes, which is enough for change detection.
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(doc).unwrap_or_default());
    hex::encode(hasher.finalize())
}

/// Excerpt used in review feeds and notification copy.
pub fn excerpt(doc: &Value, max_chars: usize) -> String {
    let text = plain_text(doc).replace('\n', " ");
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut s: String = text.chars().take(max_chars).collect();
    s.push('…');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn plain_text_puts_each_block_on_its_own_line() {
        let value = doc(r#"{"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"One."}]},
                {"type":"paragraph","content":[{"type":"text","text":"Two."}]}
            ]}"#);
        assert_eq!(plain_text(&value), "One.\nTwo.");
        assert_eq!(word_count(&value), 2);
    }

    #[test]
    fn marks_do_not_affect_the_flattened_text() {
        let plain = doc(r#"{"type":"doc","content":[{"type":"paragraph","content":[
            {"type":"text","text":"a bold word"}]}]}"#);
        let marked = doc(r#"{"type":"doc","content":[{"type":"paragraph","content":[
            {"type":"text","text":"a "},
            {"type":"text","text":"bold","marks":[{"type":"bold"}]},
            {"type":"text","text":" word"}]}]}"#);
        assert_eq!(plain_text(&plain), plain_text(&marked));
    }

    #[test]
    fn hard_breaks_become_newlines() {
        let value = doc(r#"{"type":"doc","content":[{"type":"paragraph","content":[
            {"type":"text","text":"one"},{"type":"hardBreak"},{"type":"text","text":"two"}]}]}"#);
        assert_eq!(plain_text(&value), "one\ntwo");
    }

    #[test]
    fn validate_rejects_anything_that_is_not_a_doc() {
        assert!(validate(&empty_doc()).is_ok());
        assert!(validate(&doc(r#"{"type":"paragraph"}"#)).is_err());
        assert!(validate(&doc(r#"{"content":[]}"#)).is_err());
    }

    #[test]
    fn content_hash_changes_only_when_the_content_does() {
        let a = doc_from_text("the same words");
        let b = doc_from_text("the same words");
        let c = doc_from_text("different words");
        assert_eq!(content_hash(&a), content_hash(&b));
        assert_ne!(content_hash(&a), content_hash(&c));
    }

    #[test]
    fn excerpt_truncates_on_a_character_boundary() {
        let value = doc_from_text("héllo wörld, this is a longer passage than we want");
        let short = excerpt(&value, 10);
        assert_eq!(short.chars().count(), 11); // 10 chars plus the ellipsis
        assert!(short.ends_with('…'));
    }

    #[test]
    fn round_trip_through_text_preserves_paragraphs() {
        let value = doc_from_text("First para.\n\nSecond para.");
        assert_eq!(value["content"].as_array().unwrap().len(), 2);
        assert_eq!(plain_text(&value), "First para.\nSecond para.");
    }
}

/// The document shape, declared for the generated schema.
///
/// The server stores and forwards scene bodies as opaque `serde_json::Value` —
/// the editor owns the schema, and the server has no reason to validate beyond
/// `validate()` above. But "opaque" would generate as `unknown` in the clients,
/// which is exactly the kind of untyped hole this contract exists to close, so
/// the fields carrying a document declare this type instead.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct DocNode {
    #[serde(rename = "type")]
    pub r#type: String,
    #[schema(no_recursion)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<DocNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marks: Option<Vec<DocMark>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attrs: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct DocMark {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attrs: Option<serde_json::Value>,
}
