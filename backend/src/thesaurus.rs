//! A thesaurus, served from this process.
//!
//! The words come from WordNet, boiled down by `scripts/build_thesaurus.py`
//! into one gzipped line per word and embedded in the binary. Looking a word up
//! never leaves the server: a writer's vocabulary — what they reach for, what
//! they are trying to replace — is as much theirs as the manuscript is.
//!
//! The file is unpacked the first time anyone asks, and kept: about thirteen
//! megabytes of text and an index into it, which is cheaper than a second
//! process or a table nobody queries any other way.
use std::collections::HashMap;
use std::io::Read;
use std::sync::OnceLock;

use serde::Serialize;

static DATA: &[u8] = include_bytes!("../assets/thesaurus.tsv.gz");

struct Thesaurus {
    text: String,
    /// Lower-cased word -> where its lines sit in `text`. More than one when
    /// WordNet holds the word in two cases ("March" and "march").
    index: HashMap<String, Vec<(usize, usize)>>,
}

fn thesaurus() -> &'static Thesaurus {
    static CELL: OnceLock<Thesaurus> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut text = String::new();
        flate2::read::GzDecoder::new(DATA)
            .read_to_string(&mut text)
            .expect("the embedded thesaurus is readable");
        let mut index: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        let mut start = 0;
        for line in text.split_inclusive('\n') {
            let end = start + line.trim_end_matches('\n').len();
            if let Some(tab) = line.find('\t') {
                index
                    .entry(line[..tab].to_lowercase())
                    .or_default()
                    .push((start, end));
            }
            start += line.len();
        }
        Thesaurus { text, index }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PartOfSpeech {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

/// One meaning of the word, and the words for it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Sense {
    pub part_of_speech: PartOfSpeech,
    /// What this sense means, briefly, to tell it from the others.
    pub gloss: String,
    /// Words with this same meaning.
    pub synonyms: Vec<String>,
    /// Near it: similar words, and more particular ones ("walk" → "march").
    pub related: Vec<String>,
    pub antonyms: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThesaurusEntry {
    /// The word as asked for.
    pub word: String,
    /// The form it was found under: "walked" is found as "walk". When the word
    /// is both a word and a form of another ("running"), this is the other,
    /// and its senses come first.
    pub base: String,
    /// Most common meaning first.
    pub senses: Vec<Sense>,
}

/// The longest word worth looking up; anything longer is not a word.
pub const MAX_WORD: usize = 64;

pub fn lookup(word: &str) -> Option<ThesaurusEntry> {
    let asked = word.trim();
    if asked.is_empty() || asked.len() > MAX_WORD {
        return None;
    }
    let book = thesaurus();
    let lower = asked.to_lowercase().replace('’', "'");
    // The word itself, and the word it is a form of: "running" is a noun in
    // its own right, but in "she was running" it is "run" that is wanted.
    let exact = book.index.contains_key(&lower).then(|| lower.clone());
    let inflected = base_forms(&lower)
        .into_iter()
        .find(|candidate| *candidate != lower && book.index.contains_key(candidate));
    let base = inflected.clone().or_else(|| exact.clone())?;
    let mut senses: Vec<Sense> = inflected
        .iter()
        .chain(exact.iter())
        .flat_map(|w| &book.index[w])
        .flat_map(|&(start, end)| book.text[start..end].split('\t').skip(1))
        .filter_map(parse_sense)
        .collect();
    // WordNet lists nouns first; an -ing or -ed form is far more often a verb.
    if inflected.is_some() && (lower.ends_with("ing") || lower.ends_with("ed")) {
        senses.sort_by_key(|s| s.part_of_speech != PartOfSpeech::Verb);
    }
    Some(ThesaurusEntry {
        word: asked.to_string(),
        base,
        senses,
    })
}

fn parse_sense(field: &str) -> Option<Sense> {
    let mut parts = field.splitn(5, ';');
    let part_of_speech = match parts.next()? {
        "n" => PartOfSpeech::Noun,
        "v" => PartOfSpeech::Verb,
        "a" => PartOfSpeech::Adjective,
        "r" => PartOfSpeech::Adverb,
        _ => return None,
    };
    let list = |s: Option<&str>| -> Vec<String> {
        s.unwrap_or("")
            .split('|')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    Some(Sense {
        part_of_speech,
        gloss: parts.next()?.to_string(),
        synonyms: list(parts.next()),
        related: list(parts.next()),
        antonyms: list(parts.next()),
    })
}

/// The forms a word might be an inflection of, after WordNet's own rules for
/// taking endings off: "houses" → "house", "running" → "run", "brighter" →
/// "bright". Candidates only; the caller keeps the first one the thesaurus has.
fn base_forms(word: &str) -> Vec<String> {
    const RULES: &[(&str, &str)] = &[
        // nouns
        ("ses", "s"),
        ("xes", "x"),
        ("zes", "z"),
        ("ches", "ch"),
        ("shes", "sh"),
        ("men", "man"),
        ("ies", "y"),
        // verbs
        ("es", "e"),
        ("es", ""),
        ("ed", "e"),
        ("ed", ""),
        ("ing", "e"),
        ("ing", ""),
        ("s", ""),
        // adjectives
        ("er", ""),
        ("est", ""),
        ("er", "e"),
        ("est", "e"),
        ("ier", "y"),
        ("iest", "y"),
        // possessives
        ("'s", ""),
        ("s'", "s"),
    ];
    let mut out = Vec::new();
    for (suffix, replacement) in RULES {
        if let Some(stem) = word.strip_suffix(suffix) {
            if stem.is_empty() {
                continue;
            }
            out.push(format!("{stem}{replacement}"));
            // "stopped" → "stopp" → "stop"; "bigger" → "bigg" → "big".
            let bytes = stem.as_bytes();
            if replacement.is_empty()
                && bytes.len() >= 3
                && bytes[bytes.len() - 1] == bytes[bytes.len() - 2]
                && !b"aeiou".contains(&bytes[bytes.len() - 1])
            {
                out.push(stem[..stem.len() - 1].to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_comes_back_with_its_senses() {
        let entry = lookup("bright").unwrap();
        assert_eq!(entry.base, "bright");
        assert!(entry.senses.len() > 3);
        let all: Vec<&str> = entry
            .senses
            .iter()
            .flat_map(|s| s.synonyms.iter().chain(&s.related))
            .map(String::as_str)
            .collect();
        assert!(all.contains(&"brilliant"));
        assert!(entry
            .senses
            .iter()
            .any(|s| s.antonyms.iter().any(|a| a == "dull")));
    }

    #[test]
    fn an_inflected_word_is_found_under_its_base() {
        assert_eq!(lookup("walked").unwrap().base, "walk");
        let running = lookup("Running").unwrap();
        assert_eq!(running.base, "run");
        assert_eq!(running.senses[0].part_of_speech, PartOfSpeech::Verb);
        assert_eq!(lookup("houses").unwrap().base, "house");
        assert_eq!(lookup("brighter").unwrap().base, "bright");
        assert_eq!(lookup("stopped").unwrap().base, "stop");
        // The word as asked is kept, for the client to match its case.
        assert_eq!(lookup("Running").unwrap().word, "Running");
    }

    #[test]
    fn a_verb_offers_the_more_particular_ways_of_doing_it() {
        let entry = lookup("walk").unwrap();
        let verbs: Vec<&Sense> = entry
            .senses
            .iter()
            .filter(|s| s.part_of_speech == PartOfSpeech::Verb)
            .collect();
        assert!(verbs
            .iter()
            .any(|s| s.related.iter().any(|w| w == "march" || w == "pace")));
    }

    #[test]
    fn nonsense_and_overlong_words_find_nothing() {
        assert!(lookup("qzxvbnm").is_none());
        assert!(lookup("").is_none());
        assert!(lookup(&"a".repeat(MAX_WORD + 1)).is_none());
    }
}
