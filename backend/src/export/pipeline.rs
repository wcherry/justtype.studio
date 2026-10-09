//! The stages between "the book as stored" and "the book as written out".
//!
//! resolve (the route loads the text) → [`select`] what is included →
//! [`build`] blocks, notes and numbering → render → validate. The pure stages
//! live here so preflight can run them and stop, and so each can be tested
//! without a database or a file in sight.
use serde::Serialize;
use serde_json::Value;

use super::options::{CommentsMode, ExportOptions, Format, NotePlacement, Numbering, Scope};
use super::{
    positioned_blocks, Block, ChapterContent, Note, NoteLayout, Role, SceneContent, Span,
};

/// One scene as stored: its document, not yet flattened.
#[derive(Debug, Clone)]
pub struct SourceScene {
    pub id: String,
    pub title: String,
    pub doc: Value,
}

#[derive(Debug, Clone)]
pub struct SourceChapter {
    pub id: String,
    pub title: String,
    pub role: Role,
    pub scenes: Vec<SourceScene>,
}

/// One of the writer's own comments. Positions are editor positions in the
/// working draft; in a version they are only a hint, since the frozen text is
/// not the text they were made against.
#[derive(Debug, Clone)]
pub struct SourceComment {
    pub scene_id: String,
    pub from: i32,
    pub to: i32,
    pub quote: String,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    pub chapter_id: String,
    pub scene_id: Option<String>,
}

/// A thing worth knowing before the file goes out. `code` is stable: the web
/// app keys on it, so a message can be reworded without breaking anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    pub code: String,
    /// Says what to do next, not just what went wrong.
    pub message: String,
    pub location: Option<Location>,
}

impl Issue {
    fn new(severity: Severity, code: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            location: None,
        }
    }
    fn at(mut self, chapter_id: &str, scene_id: Option<&str>) -> Self {
        if !chapter_id.is_empty() {
            self.location = Some(Location {
                chapter_id: chapter_id.into(),
                scene_id: scene_id.map(str::to_string),
            });
        }
        self
    }
}

/// What is going out, in numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub chapters: usize,
    pub scenes: usize,
    pub words: usize,
    pub comments: usize,
}

pub struct Built {
    pub chapters: Vec<ChapterContent>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
    pub note_layout: NoteLayout,
}

/// Stage 1: which chapters and scenes are in.
pub fn select(chapters: Vec<SourceChapter>, options: &ExportOptions) -> Vec<SourceChapter> {
    let front = options.content.front_matter;
    let back = options.include_back_matter();
    chapters
        .into_iter()
        .filter(|chapter| match chapter.role {
            Role::Front => front,
            Role::Back => back,
            Role::Chapter => true,
        })
        .filter_map(|mut chapter| match &options.content.scope {
            Scope::All => Some(chapter),
            Scope::Selected {
                chapter_ids,
                scene_ids,
            } => {
                if chapter_ids.contains(&chapter.id) {
                    return Some(chapter);
                }
                chapter.scenes.retain(|scene| scene_ids.contains(&scene.id));
                (!chapter.scenes.is_empty()).then_some(chapter)
            }
        })
        .collect()
}

fn spans_of(block: &Block) -> Option<&Vec<Span>> {
    match block {
        Block::Paragraph(spans) | Block::Quote(spans) => Some(spans),
        Block::Heading { spans, .. } => Some(spans),
        Block::SceneBreak => None,
    }
}

fn spans_mut(block: &mut Block) -> Option<&mut Vec<Span>> {
    match block {
        Block::Paragraph(spans) | Block::Quote(spans) => Some(spans),
        Block::Heading { spans, .. } => Some(spans),
        Block::SceneBreak => None,
    }
}

fn text_of(block: &Block) -> Vec<char> {
    spans_of(block)
        .map(|spans| spans.iter().flat_map(|s| s.text.chars()).collect())
        .unwrap_or_default()
}

fn find_all(haystack: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    (0..=haystack.len() - needle.len())
        .filter(|&i| haystack[i..i + needle.len()] == *needle)
        .collect()
}

/// Where a comment's mark goes: a block and a character offset into its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub block: usize,
    pub offset: usize,
    pub placed: bool,
}

/// Finds the end of the passage a comment was made on.
///
/// In a working draft the stored positions are tried first and trusted only
/// if the text under them is still the quote; otherwise, and always in a
/// frozen version, the quote is looked for in the scene (nearest to the old
/// position when there are several). A comment whose quote has gone is put at
/// the end of its scene and reported, not dropped.
pub fn anchor(blocks: &[(Block, u32)], comment: &SourceComment, use_positions: bool) -> Anchor {
    let texts: Vec<Vec<char>> = blocks.iter().map(|(b, _)| text_of(b)).collect();
    let quote: Vec<char> = comment.quote.chars().collect();

    let past_word = |block: usize, mut offset: usize| {
        // A mark inside a word goes after the word.
        let text = &texts[block];
        if offset > 0 && offset < text.len() && text[offset - 1].is_alphanumeric() {
            while offset < text.len() && text[offset].is_alphanumeric() {
                offset += 1;
            }
        }
        offset
    };

    if use_positions && comment.to >= comment.from && comment.from >= 0 {
        for (index, (_, start)) in blocks.iter().enumerate() {
            let start = *start as i64;
            let text = &texts[index];
            let len16: i64 = text.iter().map(|c| c.len_utf16() as i64).sum();
            let (from, to) = (comment.from as i64, comment.to as i64);
            if from >= start && to <= start + len16 {
                // Positions count UTF-16 units; the text is held in chars.
                let to_char = |units: i64| {
                    let mut seen = 0;
                    for (i, c) in text.iter().enumerate() {
                        if seen >= units {
                            return i;
                        }
                        seen += c.len_utf16() as i64;
                    }
                    text.len()
                };
                let (a, b) = (to_char(from - start), to_char(to - start));
                if a <= b && text[a..b] == quote[..] {
                    return Anchor {
                        block: index,
                        offset: past_word(index, b),
                        placed: true,
                    };
                }
            }
        }
    }

    let nearest = |needle: &[char]| -> Option<(usize, usize)> {
        let mut best: Option<(i64, usize, usize)> = None;
        for (index, text) in texts.iter().enumerate() {
            for at in find_all(text, needle) {
                let distance = if use_positions {
                    (blocks[index].1 as i64 + at as i64 - comment.from as i64).abs()
                } else {
                    // Without usable positions, the first one wins.
                    (index * 1_000_000 + at) as i64
                };
                if best.map_or(true, |(d, ..)| distance < d) {
                    best = Some((distance, index, at + needle.len()));
                }
            }
        }
        best.map(|(_, index, end)| (index, end))
    };

    if let Some((block, end)) = nearest(&quote) {
        return Anchor {
            block,
            offset: past_word(block, end),
            placed: true,
        };
    }
    // A passage that crossed paragraphs is stored with line breaks in it; the
    // last line is where the mark belongs.
    if let Some(last) = comment
        .quote
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .last()
    {
        let last: Vec<char> = last.chars().collect();
        if last != quote {
            if let Some((block, end)) = nearest(&last) {
                return Anchor {
                    block,
                    offset: past_word(block, end),
                    placed: true,
                };
            }
        }
    }

    let block = texts.iter().rposition(|t| !t.is_empty()).unwrap_or(0);
    Anchor {
        block,
        offset: texts.get(block).map_or(0, Vec::len),
        placed: false,
    }
}

/// Splits runs at the given character offsets and drops a note reference in at
/// each. Marks sharing an offset keep their order.
fn insert_marks(spans: Vec<Span>, marks: &[(usize, u32)]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut pending: Vec<(usize, u32)> = marks.to_vec();
    pending.sort_by_key(|(offset, _)| *offset);
    let mut pending = pending.into_iter().peekable();
    let mark = |n: u32| Span {
        note: Some(n),
        ..Span::default()
    };

    while let Some((_, n)) = pending.next_if(|(offset, _)| *offset == 0) {
        out.push(mark(n));
    }
    let mut position = 0;
    for span in spans {
        let chars: Vec<char> = span.text.chars().collect();
        let end = position + chars.len();
        let mut cursor = 0;
        while let Some((offset, n)) = pending.next_if(|(o, _)| *o > position && *o <= end) {
            let cut = offset - position;
            if cut > cursor {
                out.push(Span {
                    text: chars[cursor..cut].iter().collect(),
                    ..span.clone()
                });
                cursor = cut;
            }
            out.push(mark(n));
        }
        if cursor < chars.len() {
            out.push(Span {
                text: chars[cursor..].iter().collect(),
                ..span.clone()
            });
        }
        position = end;
    }
    // Anything past the end (an empty block) goes last.
    for (_, n) in pending {
        out.push(mark(n));
    }
    out
}

fn word_count(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .filter_map(spans_of)
        .map(|spans| {
            spans
                .iter()
                .map(|s| s.text.split_whitespace().count())
                .sum::<usize>()
        })
        .sum()
}

/// Stage 2: flatten the chosen text into blocks, place the writer's comments
/// as notes, and number them. Empty scenes and chapters fall away here, each
/// reported.
pub fn build(
    chapters: Vec<SourceChapter>,
    comments: &[SourceComment],
    options: &ExportOptions,
    use_positions: bool,
) -> Built {
    let mut issues = Vec::new();
    let mut summary = Summary::default();
    let want_notes = options.content.comments != CommentsMode::Off;
    let quote_always = options.quote_passage();
    let per_chapter = options.content.comment_numbering == Numbering::Chapter;

    let mut out: Vec<ChapterContent> = Vec::new();
    let mut counter: u32 = 1;

    for chapter in chapters {
        let mut scenes: Vec<SceneContent> = Vec::new();
        // (scene index, block index, offset, comment) in reading order.
        let mut placements: Vec<(usize, usize, usize, &SourceComment, bool)> = Vec::new();

        for scene in &chapter.scenes {
            let positioned = positioned_blocks(&scene.doc);
            if positioned.is_empty() {
                continue;
            }
            let scene_index = scenes.len();
            if want_notes {
                for comment in comments
                    .iter()
                    .filter(|c| c.scene_id == scene.id && !c.note.trim().is_empty())
                {
                    let found = anchor(&positioned, comment, use_positions);
                    if !found.placed {
                        issues.push(
                            Issue::new(
                                Severity::Warning,
                                "comment_unplaced",
                                "A comment could not be placed beside its passage, because the \
                                 text has changed. It is at the end of its scene instead.",
                            )
                            .at(&chapter.id, Some(&scene.id)),
                        );
                    }
                    placements.push((
                        scene_index,
                        found.block,
                        found.offset,
                        comment,
                        found.placed,
                    ));
                }
            }
            scenes.push(SceneContent {
                id: scene.id.clone(),
                title: scene.title.clone(),
                blocks: positioned.into_iter().map(|(b, _)| b).collect(),
            });
        }

        if scenes.is_empty() {
            if !chapter.scenes.is_empty() || chapter.role == Role::Chapter {
                issues.push(
                    Issue::new(
                        Severity::Warning,
                        "chapter_empty",
                        format!(
                            "“{}” has no text and will be left out. Write something in it, or \
                             leave it out of the selection.",
                            chapter.title
                        ),
                    )
                    .at(&chapter.id, None),
                );
            }
            continue;
        }

        if per_chapter {
            counter = 1;
        }
        placements.sort_by_key(|(scene, block, offset, ..)| (*scene, *block, *offset));
        let mut notes = Vec::new();
        let mut marks: std::collections::BTreeMap<(usize, usize), Vec<(usize, u32)>> =
            Default::default();
        for (scene, block, offset, comment, placed) in placements {
            let number = counter;
            counter += 1;
            marks.entry((scene, block)).or_default().push((offset, number));
            notes.push(Note {
                number,
                text: comment.note.trim().to_string(),
                quote: (quote_always || !placed).then(|| comment.quote.clone()),
            });
        }
        // All of a block's marks go in together, so inserting one does not
        // shift the offset of the next.
        for ((scene, block), at) in marks {
            if let Some(spans) = scenes[scene].blocks.get_mut(block).and_then(spans_mut) {
                let existing = std::mem::take(spans);
                *spans = insert_marks(existing, &at);
            }
        }

        summary.chapters += 1;
        summary.scenes += scenes.len();
        summary.comments += notes.len();
        summary.words += scenes.iter().map(|s| word_count(&s.blocks)).sum::<usize>();
        out.push(ChapterContent {
            id: chapter.id,
            title: chapter.title,
            role: chapter.role,
            scenes,
            notes,
        });
    }

    let note_layout = match options.content.comments {
        CommentsMode::Off => NoteLayout::None,
        CommentsMode::Footnotes if options.format == Format::Pdf => {
            issues.push(Issue::new(
                Severity::Info,
                "pdf_footnotes_as_endnotes",
                "PDF puts comments at the end of each chapter rather than at the foot of the \
                 page. Use DOCX for true footnotes.",
            ));
            NoteLayout::EndnotesChapter
        }
        CommentsMode::Footnotes => NoteLayout::Footnotes,
        CommentsMode::Endnotes {
            placement: NotePlacement::Chapter,
        } => NoteLayout::EndnotesChapter,
        CommentsMode::Endnotes {
            placement: NotePlacement::Book,
        } => NoteLayout::EndnotesBook,
    };
    if want_notes {
        issues.push(Issue::new(
            Severity::Info,
            "comments_included",
            "Your comments will be in this file, readable by anyone you send it to.",
        ));
    }

    Built {
        chapters: out,
        issues,
        summary,
        note_layout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(paragraphs: &[&str]) -> Value {
        json!({"type":"doc","content": paragraphs.iter().map(|p| json!({
            "type":"paragraph","content":[{"type":"text","text":p}]
        })).collect::<Vec<_>>()})
    }

    fn chapter(id: &str, role: Role, scenes: Vec<(&str, Value)>) -> SourceChapter {
        SourceChapter {
            id: id.into(),
            title: format!("Chapter {id}"),
            role,
            scenes: scenes
                .into_iter()
                .map(|(sid, doc)| SourceScene {
                    id: sid.into(),
                    title: sid.into(),
                    doc,
                })
                .collect(),
        }
    }

    #[test]
    fn selection_follows_the_outline_whatever_order_the_boxes_were_ticked() {
        let all = vec![
            chapter("1", Role::Chapter, vec![("a", doc(&["x"]))]),
            chapter("2", Role::Chapter, vec![("b", doc(&["y"])), ("c", doc(&["z"]))]),
            chapter("3", Role::Chapter, vec![("d", doc(&["w"]))]),
        ];
        let mut options = ExportOptions::default();
        options.content.scope = Scope::Selected {
            chapter_ids: vec!["3".into(), "1".into()],
            scene_ids: vec!["c".into()],
        };
        let picked = select(all, &options);
        let ids: Vec<&str> = picked.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3"]);
        assert_eq!(picked[1].scenes.len(), 1);
        assert_eq!(picked[1].scenes[0].id, "c");
    }

    #[test]
    fn selecting_nothing_selects_nothing() {
        let all = vec![chapter("1", Role::Chapter, vec![("a", doc(&["x"]))])];
        let mut options = ExportOptions::default();
        options.content.scope = Scope::Selected {
            chapter_ids: vec![],
            scene_ids: vec![],
        };
        assert!(select(all, &options).is_empty());
    }

    #[test]
    fn front_and_back_matter_follow_their_toggles() {
        let all = || {
            vec![
                chapter("f", Role::Front, vec![("a", doc(&["x"]))]),
                chapter("1", Role::Chapter, vec![("b", doc(&["y"]))]),
                chapter("z", Role::Back, vec![("c", doc(&["w"]))]),
            ]
        };
        let mut options = ExportOptions::default();
        // The manuscript preset leaves back matter out unless asked.
        let ids = |o: &ExportOptions| -> Vec<String> {
            select(all(), o).into_iter().map(|c| c.id).collect()
        };
        assert_eq!(ids(&options), ["f", "1"]);
        options.content.back_matter = Some(true);
        assert_eq!(ids(&options), ["f", "1", "z"]);
        options.content.front_matter = false;
        assert_eq!(ids(&options), ["1", "z"]);
    }

    #[test]
    fn empty_scenes_and_chapters_are_dropped_and_reported() {
        let all = vec![
            chapter("1", Role::Chapter, vec![("a", doc(&["Hello there."]))]),
            chapter("2", Role::Chapter, vec![("b", json!({"type":"doc","content":[]}))]),
        ];
        let built = build(all, &[], &ExportOptions::default(), true);
        assert_eq!(built.chapters.len(), 1);
        assert_eq!(built.summary.words, 2);
        assert!(built.issues.iter().any(|i| i.code == "chapter_empty"));
    }
}
