//! Names for chapters and scenes the writer has not named.
//!
//! A new chapter or scene is stored without a title. Wherever one has to be
//! shown it is called by where it falls — "Chapter 3", "Scene 2" — so the
//! name follows it when it moves, and a title the writer types replaces it.
//! A version freezes these names as they were when it was taken, so a
//! published book never has an empty heading.

/// `title` if the writer gave one, otherwise "Chapter `number`" (1-based).
pub fn chapter_name(title: &str, number: usize) -> String {
    named_or(title, "Chapter", number)
}

/// `title` if the writer gave one, otherwise "Part `number`" (1-based,
/// counted among the parts only).
pub fn part_name(title: &str, number: usize) -> String {
    named_or(title, "Part", number)
}

/// `title` if the writer gave one, otherwise "Scene `number`" (1-based,
/// counted within its chapter).
pub fn scene_name(title: &str, number: usize) -> String {
    named_or(title, "Scene", number)
}

/// A scene's name in a chapter of `role`: in a part, an untitled scene is its
/// "Part page".
pub fn scene_name_in(role: &str, title: &str, number: usize) -> String {
    match (role, number) {
        ("part", 1) if title.trim().is_empty() => "Part page".into(),
        ("part", _) => named_or(title, "Part page", number),
        _ => scene_name(title, number),
    }
}

/// Every chapter's name, from `(title, role)` in book order. Front and back
/// matter are not numbered, and do not take a number from the chapters
/// around them: "Chapter 4" is the fourth chapter. Parts are counted on
/// their own, and chapter numbers run on through them.
pub fn chapter_names<'a>(chapters: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut numbered = 0;
    let mut parts = 0;
    chapters
        .into_iter()
        .map(|(title, role)| match role {
            "part" => {
                parts += 1;
                part_name(title, parts)
            }
            "front" | "back" if !title.trim().is_empty() => title.trim().to_string(),
            "front" => "Front matter".into(),
            "back" => "Back matter".into(),
            _ => {
                numbered += 1;
                chapter_name(title, numbered)
            }
        })
        .collect()
}

fn named_or(title: &str, kind: &str, number: usize) -> String {
    let title = title.trim();
    if title.is_empty() {
        format!("{kind} {number}")
    } else {
        title.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_wins() {
        assert_eq!(chapter_name("The Siege", 3), "The Siege");
        assert_eq!(scene_name("  Dawn ", 1), "Dawn");
    }

    #[test]
    fn no_title_is_named_by_place() {
        assert_eq!(chapter_name("", 3), "Chapter 3");
        assert_eq!(scene_name("   ", 2), "Scene 2");
    }

    #[test]
    fn parts_and_matter_keep_out_of_the_chapter_count() {
        let names = chapter_names([
            ("", "front"),
            ("", "part"),
            ("", "chapter"),
            ("The Door", "chapter"),
            ("Interlude", "part"),
            ("", "chapter"),
            ("", "part"),
            ("Thanks", "back"),
        ]);
        assert_eq!(
            names,
            [
                "Front matter",
                "Part 1",
                "Chapter 1",
                "The Door",
                "Interlude",
                "Chapter 3",
                "Part 3",
                "Thanks"
            ]
        );
        assert_eq!(scene_name_in("part", "", 1), "Part page");
        assert_eq!(scene_name_in("part", "", 2), "Part page 2");
        assert_eq!(scene_name_in("part", "Epigraph", 1), "Epigraph");
        assert_eq!(scene_name_in("chapter", "", 1), "Scene 1");
    }
}
