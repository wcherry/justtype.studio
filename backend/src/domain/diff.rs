//! Version-to-version comparison. Diffs are computed over the flattened text of
//! each scene, which keeps them readable for authors and cheap to render on both
//! clients.
use serde::Serialize;
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Equal,
    Insert,
    Delete,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TextChange {
    pub op: Op,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SceneChangeKind {
    Added,
    Removed,
    Modified,
    Moved,
    Unchanged,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SceneDiff {
    pub scene_id: String,
    pub scene_title: String,
    pub chapter_title: String,
    pub kind: SceneChangeKind,
    pub words_before: i32,
    pub words_after: i32,
    /// Paragraph by paragraph, for a modified scene.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<BlockDiff>,
}

#[derive(Debug, Clone, Serialize, Default, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub scenes_added: usize,
    pub scenes_removed: usize,
    pub scenes_modified: usize,
    pub scenes_moved: usize,
    pub paragraphs_changed: usize,
    pub words_added: i32,
    pub words_removed: i32,
}

/// One paragraph's fate between two versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum BlockChangeKind {
    Unchanged,
    /// Same paragraph, edited. `changes` carries the word-level detail.
    Modified,
    Added,
    Removed,
    /// The same text, somewhere else. Reported as a move rather than as a
    /// deletion and an insertion, because "I moved this paragraph" is what the
    /// writer did and what they need to see.
    Moved,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockDiff {
    pub kind: BlockChangeKind,
    /// Index of this paragraph in the earlier version, when it was in it.
    pub before_index: Option<usize>,
    pub after_index: Option<usize>,
    pub before: String,
    pub after: String,
    /// Word-level detail, for a modified paragraph.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<TextChange>,
}

/// Below this, two paragraphs are different paragraphs rather than one edited.
///
/// Chosen by what it produces: prose paragraphs share a lot of short words, so
/// a permissive threshold pairs unrelated paragraphs that happen to both
/// contain "the" and "which". At 0.6 a rewritten sentence still reads as an
/// edit of the original and an unrelated paragraph does not.
const SAME_PARAGRAPH: f32 = 0.6;

/// How alike two paragraphs are, from 0 to 1.
fn similarity(before: &str, after: &str) -> f32 {
    if before == after {
        return 1.0;
    }
    if before.is_empty() || after.is_empty() {
        return 0.0;
    }
    TextDiff::from_words(before, after).ratio()
}

/// Aligns two documents paragraph by paragraph.
///
/// A whole scene diffed as one run of words is unreadable past a few
/// sentences: the changes smear across paragraph boundaries and every reflowed
/// line looks edited. Aligning first means a writer sees "this paragraph
/// changed, these two did not, that one moved".
///
/// Order matters. Exact matches are resolved before any similarity pairing,
/// because an identical paragraph elsewhere is a *certainty* that it moved,
/// while similarity is a guess — and letting a guess consume a paragraph that
/// certainly moved produces the worst of both: a spurious edit and a spurious
/// insertion.
pub fn diff_blocks(before: &[String], after: &[String]) -> Vec<BlockDiff> {
    use similar::{capture_diff_slices, Algorithm, DiffOp};

    let mut out: Vec<BlockDiff> = Vec::new();
    // Indices not accounted for by an unchanged run, which are what the rest of
    // the work is about.
    let mut old_loose: Vec<usize> = Vec::new();
    let mut new_loose: Vec<usize> = Vec::new();

    for op in capture_diff_slices(Algorithm::Myers, before, after) {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for offset in 0..len {
                    out.push(BlockDiff {
                        kind: BlockChangeKind::Unchanged,
                        before_index: Some(old_index + offset),
                        after_index: Some(new_index + offset),
                        before: before[old_index + offset].clone(),
                        after: after[new_index + offset].clone(),
                        changes: Vec::new(),
                    });
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                old_loose.extend(old_index..old_index + old_len);
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                new_loose.extend(new_index..new_index + new_len);
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                old_loose.extend(old_index..old_index + old_len);
                new_loose.extend(new_index..new_index + new_len);
            }
        }
    }

    // 1. Exact matches: the paragraph did not change, it moved.
    let mut matched_old: Vec<bool> = vec![false; old_loose.len()];
    let mut matched_new: Vec<bool> = vec![false; new_loose.len()];

    for (old_slot, old_index) in old_loose.iter().enumerate() {
        let Some(new_slot) = new_loose.iter().enumerate().position(|(slot, new_index)| {
            !matched_new[slot] && after[*new_index] == before[*old_index]
        }) else {
            continue;
        };
        matched_old[old_slot] = true;
        matched_new[new_slot] = true;
        out.push(BlockDiff {
            kind: BlockChangeKind::Moved,
            before_index: Some(*old_index),
            after_index: Some(new_loose[new_slot]),
            before: before[*old_index].clone(),
            after: before[*old_index].clone(),
            changes: Vec::new(),
        });
    }

    // 2. Of what is left, pair each old paragraph with the new one it most
    //    resembles, if any resembles it enough.
    for (old_slot, old_index) in old_loose.iter().enumerate() {
        if matched_old[old_slot] {
            continue;
        }
        let best = new_loose
            .iter()
            .enumerate()
            .filter(|(slot, _)| !matched_new[*slot])
            .map(|(slot, new_index)| (slot, similarity(&before[*old_index], &after[*new_index])))
            .filter(|(_, score)| *score >= SAME_PARAGRAPH)
            .max_by(|a, b| a.1.total_cmp(&b.1));

        match best {
            Some((new_slot, _)) => {
                matched_old[old_slot] = true;
                matched_new[new_slot] = true;
                let new_index = new_loose[new_slot];
                out.push(BlockDiff {
                    kind: BlockChangeKind::Modified,
                    before_index: Some(*old_index),
                    after_index: Some(new_index),
                    before: before[*old_index].clone(),
                    after: after[new_index].clone(),
                    changes: diff_text(&before[*old_index], &after[new_index]),
                });
            }
            None => out.push(BlockDiff {
                kind: BlockChangeKind::Removed,
                before_index: Some(*old_index),
                after_index: None,
                before: before[*old_index].clone(),
                after: String::new(),
                changes: Vec::new(),
            }),
        }
    }

    // 3. Anything new left over is new.
    for (new_slot, new_index) in new_loose.iter().enumerate() {
        if matched_new[new_slot] {
            continue;
        }
        out.push(BlockDiff {
            kind: BlockChangeKind::Added,
            before_index: None,
            after_index: Some(*new_index),
            before: String::new(),
            after: after[*new_index].clone(),
            changes: Vec::new(),
        });
    }

    // Read top to bottom as the scene stands now; removed paragraphs, having no
    // place in it, come last.
    out.sort_by_key(|block| block.after_index.unwrap_or(usize::MAX));
    out
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

#[cfg(test)]
mod block_tests {
    use super::*;

    fn lines(text: &[&str]) -> Vec<String> {
        text.iter().map(|t| t.to_string()).collect()
    }

    fn kinds(blocks: &[BlockDiff]) -> Vec<BlockChangeKind> {
        blocks.iter().map(|b| b.kind).collect()
    }

    #[test]
    fn identical_documents_are_all_unchanged() {
        let doc = lines(&["One.", "Two.", "Three."]);
        assert_eq!(
            kinds(&diff_blocks(&doc, &doc)),
            vec![BlockChangeKind::Unchanged; 3]
        );
    }

    #[test]
    fn an_edited_paragraph_is_modified_not_replaced() {
        let before = lines(&["The cat sat on the mat.", "It was a Tuesday."]);
        let after = lines(&["The cat sat on the rug.", "It was a Tuesday."]);
        let blocks = diff_blocks(&before, &after);
        assert_eq!(
            kinds(&blocks),
            vec![BlockChangeKind::Modified, BlockChangeKind::Unchanged]
        );
        // And the word-level detail is there for the one that changed.
        assert!(blocks[0]
            .changes
            .iter()
            .any(|c| c.op == Op::Insert && c.value.contains("rug")));
    }

    #[test]
    fn a_wholly_different_paragraph_is_added_and_removed() {
        let before = lines(&["The cat sat on the mat."]);
        let after = lines(&["Artillery opened up before dawn."]);
        let blocks = diff_blocks(&before, &after);
        assert!(blocks.iter().any(|b| b.kind == BlockChangeKind::Removed));
        assert!(blocks.iter().any(|b| b.kind == BlockChangeKind::Added));
        assert!(!blocks.iter().any(|b| b.kind == BlockChangeKind::Modified));
    }

    /// The point of the exercise: moving a paragraph should say so.
    #[test]
    fn a_moved_paragraph_is_a_move_not_a_delete_and_an_insert() {
        let before = lines(&["First.", "Second.", "Third."]);
        let after = lines(&["Third.", "First.", "Second."]);
        let blocks = diff_blocks(&before, &after);
        assert_eq!(
            blocks
                .iter()
                .filter(|b| b.kind == BlockChangeKind::Moved)
                .count(),
            1
        );
        assert!(!blocks.iter().any(|b| b.kind == BlockChangeKind::Removed));
        assert!(!blocks.iter().any(|b| b.kind == BlockChangeKind::Added));
    }

    #[test]
    fn a_move_records_where_it_came_from_and_went() {
        let before = lines(&["First.", "Second.", "Third."]);
        let after = lines(&["Third.", "First.", "Second."]);
        let moved = diff_blocks(&before, &after)
            .into_iter()
            .find(|b| b.kind == BlockChangeKind::Moved)
            .expect("expected a move");
        assert_eq!(moved.before_index, Some(2));
        assert_eq!(moved.after_index, Some(0));
    }

    #[test]
    fn insertions_and_deletions_are_reported_plainly() {
        let before = lines(&["One.", "Two."]);
        let after = lines(&["One.", "One and a half.", "Two."]);
        let blocks = diff_blocks(&before, &after);
        assert_eq!(
            blocks
                .iter()
                .filter(|b| b.kind == BlockChangeKind::Added)
                .count(),
            1
        );

        let blocks = diff_blocks(&after, &before);
        assert_eq!(
            blocks
                .iter()
                .filter(|b| b.kind == BlockChangeKind::Removed)
                .count(),
            1
        );
    }

    /// Output is ordered by the new document, so it reads top to bottom as the
    /// writer sees the scene now. Removed paragraphs, having no place in it,
    /// come last.
    #[test]
    fn results_are_ordered_by_the_new_document() {
        let before = lines(&["A.", "B.", "C."]);
        let after = lines(&["A.", "C.", "D."]);
        let blocks = diff_blocks(&before, &after);
        let placed: Vec<usize> = blocks.iter().filter_map(|b| b.after_index).collect();
        let mut sorted = placed.clone();
        sorted.sort_unstable();
        assert_eq!(placed, sorted);
    }

    /// Regression: prose paragraphs share enough short words that an unrelated
    /// pair can score above the threshold. When that happened to a paragraph
    /// that had merely moved, it was consumed as an "edit", and its real
    /// counterpart then showed up as an unexplained insertion — the worst of
    /// both readings. Exact matches are resolved first for this reason.
    #[test]
    fn a_moved_paragraph_is_not_stolen_by_a_similar_looking_stranger() {
        let before = lines(&[
            "Alpha paragraph, which stays exactly as it is from first to last.",
            "Gamma paragraph, which will be moved up to the top of the scene.",
            "Delta paragraph, which will be cut entirely.",
        ]);
        let after = lines(&[
            "Gamma paragraph, which will be moved up to the top of the scene.",
            "Alpha paragraph, which stays exactly as it is from first to last.",
            "Epsilon paragraph, entirely new and owing nothing to what came before.",
        ]);
        let blocks = diff_blocks(&before, &after);

        let gamma = blocks
            .iter()
            .find(|b| b.after.starts_with("Gamma"))
            .expect("Gamma should still be accounted for");
        assert_eq!(gamma.kind, BlockChangeKind::Moved);

        assert!(blocks
            .iter()
            .any(|b| b.kind == BlockChangeKind::Added && b.after.starts_with("Epsilon")));
        assert!(blocks
            .iter()
            .any(|b| b.kind == BlockChangeKind::Removed && b.before.starts_with("Delta")));
    }

    #[test]
    fn every_paragraph_is_accounted_for_exactly_once() {
        let before = lines(&["A.", "B.", "C.", "D."]);
        let after = lines(&["C.", "A.", "B prime.", "E."]);
        let blocks = diff_blocks(&before, &after);

        let from_before: Vec<usize> = blocks.iter().filter_map(|b| b.before_index).collect();
        let from_after: Vec<usize> = blocks.iter().filter_map(|b| b.after_index).collect();
        let unique = |mut v: Vec<usize>| {
            v.sort_unstable();
            let len = v.len();
            v.dedup();
            v.len() == len
        };
        assert!(
            unique(from_before.clone()),
            "a source paragraph was used twice"
        );
        assert!(
            unique(from_after.clone()),
            "a result paragraph was used twice"
        );
        assert_eq!(from_before.len(), before.len());
        assert_eq!(from_after.len(), after.len());
    }

    #[test]
    fn an_empty_document_on_either_side_still_works() {
        let something = lines(&["Only paragraph."]);
        assert_eq!(
            kinds(&diff_blocks(&[], &something)),
            vec![BlockChangeKind::Added]
        );
        assert_eq!(
            kinds(&diff_blocks(&something, &[])),
            vec![BlockChangeKind::Removed]
        );
        assert!(diff_blocks(&[], &[]).is_empty());
    }
}
