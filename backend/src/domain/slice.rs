//! Which part of a book a scoped version, publication or review covers.
//!
//! A scope is `work`, `part`, `chapter` or `scene`, with the id of the part,
//! chapter or scene it names. A part is not a container in the data: it is a
//! chapter whose role is `part`, and it holds the chapters after it up to the
//! next part or the back matter. So what a part covers depends on the order of
//! chapters, and every caller hands that order in rather than each working it
//! out again.
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slice {
    Whole,
    /// These chapters, every scene in each.
    Chapters(HashSet<String>),
    Scene(String),
}

impl Slice {
    /// `chapters` is the book's chapters in reading order, as `(id, role)`.
    pub fn of<'a>(
        scope: &str,
        reference: Option<&str>,
        chapters: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Slice {
        match (scope, reference) {
            ("scene", Some(scene_id)) => Slice::Scene(scene_id.to_string()),
            ("chapter", Some(chapter_id)) => {
                Slice::Chapters(HashSet::from([chapter_id.to_string()]))
            }
            ("part", Some(part_id)) => {
                let mut held = HashSet::new();
                let mut inside = false;
                for (id, role) in chapters {
                    if id == part_id {
                        inside = true;
                        held.insert(id.to_string());
                    } else if inside && role == "chapter" {
                        held.insert(id.to_string());
                    } else if inside {
                        break;
                    }
                }
                Slice::Chapters(held)
            }
            _ => Slice::Whole,
        }
    }

    pub fn holds(&self, chapter_id: &str, scene_id: &str) -> bool {
        match self {
            Slice::Whole => true,
            Slice::Chapters(ids) => ids.contains(chapter_id),
            Slice::Scene(id) => id == scene_id,
        }
    }

    pub fn holds_chapter(&self, chapter_id: &str) -> bool {
        match self {
            Slice::Whole => true,
            Slice::Chapters(ids) => ids.contains(chapter_id),
            Slice::Scene(_) => false,
        }
    }
}

/// Chapters in reading order, once each, from entries that repeat a chapter
/// for every scene in it — the shape of a version's entries.
pub fn chapter_order<'a>(
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<(&'a str, &'a str)> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    for (id, role) in entries {
        if !out.iter().any(|(seen, _)| *seen == id) {
            out.push((id, role));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOK: &[(&str, &str)] = &[
        ("pre", "front"),
        ("p1", "part"),
        ("c1", "chapter"),
        ("c2", "chapter"),
        ("p2", "part"),
        ("c3", "chapter"),
        ("ack", "back"),
    ];

    #[test]
    fn a_part_holds_its_chapters_up_to_the_next_part_or_the_back_matter() {
        let one = Slice::of("part", Some("p1"), BOOK.iter().copied());
        assert_eq!(
            one,
            Slice::Chapters(["p1", "c1", "c2"].map(String::from).into())
        );
        let two = Slice::of("part", Some("p2"), BOOK.iter().copied());
        assert_eq!(two, Slice::Chapters(["p2", "c3"].map(String::from).into()));
        assert!(!two.holds_chapter("ack"));
    }

    #[test]
    fn the_other_scopes() {
        assert_eq!(Slice::of("work", None, BOOK.iter().copied()), Slice::Whole);
        let chapter = Slice::of("chapter", Some("c2"), BOOK.iter().copied());
        assert!(chapter.holds("c2", "any") && !chapter.holds("c1", "any"));
        let scene = Slice::of("scene", Some("s9"), BOOK.iter().copied());
        assert!(scene.holds("c1", "s9") && !scene.holds("c1", "s8"));
    }

    #[test]
    fn chapter_order_keeps_each_chapter_once() {
        let entries = [
            ("c1", "chapter"),
            ("c1", "chapter"),
            ("c2", "part"),
            ("c2", "part"),
        ];
        assert_eq!(chapter_order(entries), [("c1", "chapter"), ("c2", "part")]);
    }
}
