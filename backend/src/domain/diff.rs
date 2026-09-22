//! Version-to-version comparison. Diffs are computed over the flattened text of
//! each scene, which keeps them readable for authors and cheap to render on both
//! clients.
use serde::Serialize;
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextChange {
    pub op: Op,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SceneChangeKind {
    Added,
    Removed,
    Modified,
    Moved,
    Unchanged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneDiff {
    pub scene_id: String,
    pub scene_title: String,
    pub chapter_title: String,
    pub kind: SceneChangeKind,
    pub words_before: i32,
    pub words_after: i32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<TextChange>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub scenes_added: usize,
    pub scenes_removed: usize,
    pub scenes_modified: usize,
    pub scenes_moved: usize,
    pub words_added: i32,
    pub words_removed: i32,
}

/// Word-level diff of two scene bodies, collapsed so long unchanged stretches
/// do not dominate the payload.
pub fn diff_text(before: &str, after: &str) -> Vec<TextChange> {
    let diff = TextDiff::from_words(before, after);
    let mut out: Vec<TextChange> = Vec::new();
    for change in diff.iter_all_changes() {
        let op = match change.tag() {
            ChangeTag::Equal => Op::Equal,
            ChangeTag::Insert => Op::Insert,
            ChangeTag::Delete => Op::Delete,
        };
        let value = change.value().to_string();
        match out.last_mut() {
            Some(last) if last.op == op => last.value.push_str(&value),
            _ => out.push(TextChange { op, value }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(changes: &[TextChange]) -> String {
        changes
            .iter()
            .map(|c| match c.op {
                Op::Equal => c.value.clone(),
                Op::Insert => format!("[+{}]", c.value),
                Op::Delete => format!("[-{}]", c.value),
            })
            .collect()
    }

    #[test]
    fn identical_text_produces_no_changes() {
        let changes = diff_text("the same", "the same");
        assert!(changes.iter().all(|c| c.op == Op::Equal));
    }

    #[test]
    fn an_inserted_word_is_reported_as_an_insert() {
        let changes = diff_text("the field went on", "the wide field went on");
        assert_eq!(rendered(&changes), "the [+wide ]field went on");
    }

    #[test]
    fn a_deleted_word_is_reported_as_a_delete() {
        let changes = diff_text("the wide field", "the field");
        assert_eq!(rendered(&changes), "the [-wide ]field");
    }

    #[test]
    fn adjacent_changes_of_the_same_kind_are_merged() {
        // Two consecutive inserted words must not produce two insert runs.
        let changes = diff_text("a d", "a b c d");
        let inserts = changes.iter().filter(|c| c.op == Op::Insert).count();
        assert_eq!(inserts, 1);
    }
}
