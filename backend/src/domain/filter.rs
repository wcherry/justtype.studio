//! The word filter: text in, a verdict out.
//!
//! Only public text is ever passed in (Principle 1): comments, reader questions
//! and answers, Daily Prompts submissions and critiques, display names, bios,
//! *about the author*, and publication titles and blurbs. [`Surface`] is the
//! closed list of those, so there is no way to ask about a manuscript.
//!
//! Both the terms and the text are normalized the same way before they are
//! compared: Unicode NFKC, lowercase, accents removed, the usual character
//! swaps (`0→o`, `3→e`, `4→a`, `5→s`, `7→t`, `1→i`, `@→a`, `$→s`), repeated
//! letters collapsed, and separators between single letters taken out
//! (`s.p.a.m`). Matching is one Aho–Corasick automaton over all the terms,
//! rebuilt when the list changes and cached in [`FilterCache`].
//!
//! There are no regular expressions: an admin can't write one that is slow or
//! matches everything.
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use aho_corasick::{AhoCorasick, MatchKind};
use diesel::prelude::*;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::db::models::WordFilterTerm;
use crate::db::schema::{word_filter_allow, word_filter_terms};
use crate::db::Conn;
use crate::error::ApiResult;

/// Everywhere the filter may be asked about. Never private writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Surface {
    Comment,
    Question,
    Answer,
    Entry,
    Critique,
    DisplayName,
    Bio,
    AboutAuthor,
    PublicationTitle,
    Blurb,
}

impl Surface {
    pub const ALL: &'static [Surface] = &[
        Surface::Comment,
        Surface::Question,
        Surface::Answer,
        Surface::Entry,
        Surface::Critique,
        Surface::DisplayName,
        Surface::Bio,
        Surface::AboutAuthor,
        Surface::PublicationTitle,
        Surface::Blurb,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Surface::Comment => "comment",
            Surface::Question => "question",
            Surface::Answer => "answer",
            Surface::Entry => "entry",
            Surface::Critique => "critique",
            Surface::DisplayName => "display_name",
            Surface::Bio => "bio",
            Surface::AboutAuthor => "about_author",
            Surface::PublicationTitle => "publication_title",
            Surface::Blurb => "blurb",
        }
    }

    pub fn parse(value: &str) -> Option<Surface> {
        Self::ALL.iter().copied().find(|s| s.as_str() == value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilterAction {
    /// The weakest: published with the term replaced.
    Mask,
    Flag,
    Hold,
    /// The strongest: the post is refused.
    Block,
}

impl FilterAction {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "block" => Some(Self::Block),
            "hold" => Some(Self::Hold),
            "flag" => Some(Self::Flag),
            "mask" => Some(Self::Mask),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Hold => "hold",
            Self::Flag => "flag",
            Self::Mask => "mask",
        }
    }
}

pub const MASK: &str = "•••";

/// Text reduced to lowercase letters, digits and single spaces, with each
/// kept character's position in the original, so a mask can be put back.
pub struct Normalized {
    pub text: String,
    /// For each byte of `text`, the index (in chars) of the original
    /// character it came from. Spaces map to the character that became them.
    pub origin: Vec<usize>,
}

fn swap(c: char) -> char {
    match c {
        '0' => 'o',
        '1' => 'i',
        '3' => 'e',
        '4' => 'a',
        '5' => 's',
        '7' => 't',
        '@' => 'a',
        '$' => 's',
        other => other,
    }
}

pub fn normalize(input: &str) -> Normalized {
    // Pass 1: char by char, so every output char knows where it came from.
    let mut chars: Vec<(char, usize)> = Vec::new();
    for (index, original) in input.chars().enumerate() {
        for c in original.to_string().nfkc() {
            // Accents come off: decompose and drop the combining marks.
            for base in c.to_string().nfd().filter(|d| !is_combining_mark(*d)) {
                for lower in base.to_lowercase() {
                    chars.push((swap(lower), index));
                }
            }
        }
    }
    // Pass 2: letters and digits stay, everything else is a space, runs of
    // spaces become one, and a run of the same letter becomes one.
    let mut spaced: Vec<(char, usize)> = Vec::new();
    for (c, at) in chars {
        let c = if c.is_alphanumeric() { c } else { ' ' };
        match spaced.last() {
            Some(&(' ', _)) if c == ' ' => continue,
            Some(&(prev, _)) if prev == c && c != ' ' => continue,
            _ => spaced.push((c, at)),
        }
    }
    // Pass 3: `s p a m` / `s.p.a.m` — three or more single letters in a row,
    // separated only by separators in the original, are one word. Done on
    // the spaced form by joining runs of single-character tokens.
    let mut out: Vec<(char, usize)> = Vec::new();
    let tokens = split_tokens(&spaced);
    let mut i = 0;
    while i < tokens.len() {
        let mut j = i;
        while j < tokens.len() && tokens[j].len() == 1 {
            j += 1;
        }
        // "a" and "i" are words on their own: "a f.r.o.g" is "a" and "frog".
        if j - i >= 4 && matches!(tokens[i][0].0, 'a' | 'i') {
            if !out.is_empty() {
                out.push((' ', tokens[i][0].1));
            }
            out.push(tokens[i][0]);
            i += 1;
            continue;
        }
        if j - i >= 3 {
            if !out.is_empty() {
                out.push((' ', tokens[i][0].1));
            }
            for token in &tokens[i..j] {
                out.push(token[0]);
            }
            i = j;
        } else {
            if !out.is_empty() {
                out.push((' ', tokens[i][0].1));
            }
            out.extend(tokens[i].iter().copied());
            i += 1;
        }
    }
    let mut text = String::new();
    let mut origin = Vec::new();
    for (c, at) in out {
        let before = text.len();
        text.push(c);
        for _ in before..text.len() {
            origin.push(at);
        }
    }
    Normalized { text, origin }
}

fn split_tokens(chars: &[(char, usize)]) -> Vec<Vec<(char, usize)>> {
    let mut tokens: Vec<Vec<(char, usize)>> = Vec::new();
    let mut current: Vec<(char, usize)> = Vec::new();
    for &(c, at) in chars {
        if c == ' ' {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push((c, at));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// The text of a term as it is stored for matching.
pub fn normalize_term(term: &str) -> String {
    normalize(term).text
}

#[derive(Debug, Clone)]
struct Entry {
    id: String,
    word: bool,
    action: FilterAction,
    surfaces: HashSet<Surface>,
}

/// A compiled list. Cheap to share; replaced whole when the list changes.
pub struct Compiled {
    automaton: Option<AhoCorasick>,
    entries: Vec<Entry>,
    allow: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub term_id: String,
    pub action: FilterAction,
    /// Where the match was in the original text, in chars.
    pub from: usize,
    pub to: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// The strongest action among the hits, if any.
    pub action: Option<FilterAction>,
    pub hits: Vec<Hit>,
    /// The text with every `mask` hit replaced, when there was one.
    pub masked: Option<String>,
}

impl Compiled {
    #[cfg(test)]
    pub fn empty() -> Self {
        Compiled {
            automaton: None,
            entries: Vec::new(),
            allow: HashSet::new(),
        }
    }

    pub fn build(terms: &[WordFilterTerm], allow: &[String]) -> Self {
        let mut patterns = Vec::new();
        let mut entries = Vec::new();
        for term in terms {
            let normalized = normalize_term(&term.term);
            if normalized.is_empty() {
                continue;
            }
            let Some(action) = FilterAction::parse(&term.action) else {
                continue;
            };
            let surfaces: HashSet<Surface> = serde_json::from_str::<Vec<String>>(&term.applies_to)
                .unwrap_or_default()
                .iter()
                .filter_map(|s| Surface::parse(s))
                .collect();
            patterns.push(normalized);
            entries.push(Entry {
                id: term.id.clone(),
                word: term.match_kind != "substring",
                action,
                surfaces,
            });
        }
        let automaton = if patterns.is_empty() {
            None
        } else {
            AhoCorasick::builder()
                .match_kind(MatchKind::LeftmostLongest)
                .build(&patterns)
                .ok()
        };
        Compiled {
            automaton,
            entries,
            allow: allow.iter().map(|w| normalize_term(w)).collect(),
        }
    }

    /// Checks `text` as it would be posted on `surface`.
    pub fn check(&self, surface: Surface, text: &str) -> Verdict {
        let Some(automaton) = &self.automaton else {
            return Verdict::default();
        };
        if text.is_empty() {
            return Verdict::default();
        }
        let normalized = normalize(text);
        let bytes = normalized.text.as_bytes();
        let mut hits = Vec::new();
        for m in automaton.find_iter(&normalized.text) {
            let entry = &self.entries[m.pattern().as_usize()];
            if !entry.surfaces.contains(&surface) {
                continue;
            }
            let (start, end) = (m.start(), m.end());
            let before_ok = start == 0 || bytes[start - 1] == b' ';
            let after_ok = end == bytes.len() || bytes[end] == b' ';
            if entry.word && !(before_ok && after_ok) {
                continue;
            }
            // The Scunthorpe problem: a match inside a word the admin has said
            // is fine isn't a match.
            let mut from = start;
            while from > 0 && bytes[from - 1] != b' ' {
                from -= 1;
            }
            let mut to = end;
            while to < bytes.len() && bytes[to] != b' ' {
                to += 1;
            }
            if self.allow.contains(&normalized.text[from..to]) {
                continue;
            }
            let first = normalized.origin[start];
            let last = normalized.origin[end - 1];
            hits.push(Hit {
                term_id: entry.id.clone(),
                action: entry.action,
                from: first,
                to: last + 1,
            });
        }
        let action = hits.iter().map(|h| h.action).max();
        let masked = hits
            .iter()
            .any(|h| h.action == FilterAction::Mask)
            .then(|| mask(text, &hits));
        Verdict {
            action,
            hits,
            masked,
        }
    }
}

fn mask(text: &str, hits: &[Hit]) -> String {
    let mut spans: Vec<(usize, usize)> = hits
        .iter()
        .filter(|h| h.action == FilterAction::Mask)
        .map(|h| (h.from, h.to))
        .collect();
    spans.sort();
    let mut out = String::new();
    let mut next = 0;
    let mut spans = spans.into_iter().peekable();
    for (i, c) in text.chars().enumerate() {
        if i < next {
            continue;
        }
        if let Some(&(from, to)) = spans.peek() {
            if i >= from {
                out.push_str(MASK);
                next = to;
                spans.next();
                while let Some(&(f, _)) = spans.peek() {
                    if f < next {
                        spans.next();
                    } else {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The compiled list, shared by every request and rebuilt on change.
#[derive(Default)]
pub struct FilterCache {
    inner: RwLock<Option<Arc<Compiled>>>,
}

impl FilterCache {
    pub fn invalidate(&self) {
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn get(&self, conn: &mut Conn) -> ApiResult<Arc<Compiled>> {
        if let Some(c) = self
            .inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return Ok(c.clone());
        }
        let terms: Vec<WordFilterTerm> = word_filter_terms::table
            .select(WordFilterTerm::as_select())
            .load(conn)?;
        let allow: Vec<String> = word_filter_allow::table
            .select(word_filter_allow::word)
            .load(conn)?;
        let compiled = Arc::new(Compiled::build(&terms, &allow));
        *self.inner.write().unwrap_or_else(|e| e.into_inner()) = Some(compiled.clone());
        Ok(compiled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(id: &str, text: &str, kind: &str, action: &str, surfaces: &[&str]) -> WordFilterTerm {
        WordFilterTerm {
            id: id.into(),
            term: text.into(),
            normalized: normalize_term(text),
            match_kind: kind.into(),
            action: action.into(),
            applies_to: serde_json::to_string(surfaces).unwrap(),
            note: String::new(),
            created_by: "a".into(),
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn list(terms: &[WordFilterTerm], allow: &[&str]) -> Compiled {
        Compiled::build(
            terms,
            &allow.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn common_evasions_match() {
        let f = list(&[term("1", "spam", "word", "block", &["comment"])], &[]);
        for text in [
            "buy spam now",
            "buy sp4m now",
            "buy s p a m now",
            "buy s.p.a.m now",
            "buy ｓｐａｍ now",
            "buy spaaaam now",
            "buy $pam now",
            "buy SPAM now",
        ] {
            assert_eq!(
                f.check(Surface::Comment, text).action,
                Some(FilterAction::Block),
                "{text}"
            );
        }
        assert_eq!(
            f.check(Surface::Comment, "a perfectly fine note").action,
            None
        );
    }

    #[test]
    fn a_word_term_is_a_whole_word_and_a_substring_term_is_not() {
        let word = list(&[term("1", "ass", "word", "block", &["comment"])], &[]);
        assert_eq!(word.check(Surface::Comment, "a classic").action, None);
        assert_eq!(
            word.check(Surface::Comment, "the ass.").action,
            Some(FilterAction::Block)
        );
        let sub = list(&[term("1", "ass", "substring", "block", &["comment"])], &[]);
        assert_eq!(
            sub.check(Surface::Comment, "a classic").action,
            Some(FilterAction::Block)
        );
    }

    #[test]
    fn an_allowlisted_word_that_contains_a_term_is_not_caught() {
        let f = list(
            &[term("1", "cum", "substring", "block", &["comment"])],
            &["cucumber", "document"],
        );
        let plain = list(&[term("1", "cum", "substring", "block", &["comment"])], &[]);
        assert_eq!(
            plain.check(Surface::Comment, "a cucumber").action,
            Some(FilterAction::Block)
        );
        assert_eq!(f.check(Surface::Comment, "a cucumber").action, None);
        assert_eq!(
            f.check(Surface::Comment, "a cum b").action,
            Some(FilterAction::Block)
        );
    }

    #[test]
    fn a_term_only_applies_where_it_is_set() {
        let f = list(&[term("1", "spam", "word", "flag", &["comment"])], &[]);
        assert!(f.check(Surface::Comment, "spam").action.is_some());
        assert!(f.check(Surface::Critique, "spam").action.is_none());
    }

    #[test]
    fn the_strongest_action_wins_and_a_mask_keeps_the_rest_of_the_text() {
        let f = list(
            &[
                term("1", "frog", "word", "mask", &["comment"]),
                term("2", "toad", "word", "flag", &["comment"]),
            ],
            &[],
        );
        let v = f.check(Surface::Comment, "A Frog, and a toad.");
        assert_eq!(v.action, Some(FilterAction::Flag));
        assert_eq!(v.masked.as_deref(), Some("A •••, and a toad."));
        let masked_only = f.check(Surface::Comment, "a f.r.o.g sat");
        assert_eq!(masked_only.action, Some(FilterAction::Mask));
        assert_eq!(masked_only.masked.as_deref(), Some("a ••• sat"));
    }

    #[test]
    fn an_empty_list_matches_nothing_and_checking_is_fast_enough() {
        assert!(Compiled::empty()
            .check(Surface::Comment, "anything")
            .hits
            .is_empty());
        let terms: Vec<WordFilterTerm> = (0..5000)
            .map(|i| {
                term(
                    &i.to_string(),
                    &format!("zzterm{i}x"),
                    "word",
                    "flag",
                    &["comment"],
                )
            })
            .collect();
        let f = list(&terms, &[]);
        let text = "lorem ipsum dolor sit amet ".repeat(400);
        assert!(text.chars().count() >= 10_000);
        let start = std::time::Instant::now();
        for _ in 0..20 {
            f.check(Surface::Comment, &text);
        }
        let each = start.elapsed() / 20;
        // The bar is 5 ms in a release build; a debug build is several times
        // slower, so the test allows for that rather than skipping the check.
        assert!(
            each < std::time::Duration::from_millis(60),
            "{each:?} per check"
        );
    }
}
