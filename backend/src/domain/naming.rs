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

/// `title` if the writer gave one, otherwise "Scene `number`" (1-based,
/// counted within its chapter).
pub fn scene_name(title: &str, number: usize) -> String {
    named_or(title, "Scene", number)
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
}
