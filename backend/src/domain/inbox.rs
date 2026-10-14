//! What each kind of inbox item means: how it is described, what it links to,
//! and what it is called when the daily summary counts them.
//!
//! The set of kinds is open. Adding one (project comments are next) is an
//! entry in [`KINDS`] and a call to `routes::inbox::notify` where the event
//! happens; the list, the unread count and the daily summary need no change.
use serde_json::Value;

pub struct KindSpec {
    pub kind: &'static str,
    /// For the summary: "1 critique", "4 critiques".
    pub singular: &'static str,
    pub plural: &'static str,
    /// One line, given who did it and the item's detail.
    pub describe: fn(actor: &str, detail: &Value) -> String,
    /// Where the item takes you, as a path in the web app.
    pub link: fn(detail: &Value) -> String,
}

fn field<'a>(detail: &'a Value, key: &str) -> &'a str {
    detail.get(key).and_then(Value::as_str).unwrap_or("")
}

fn entry_link(detail: &Value) -> String {
    let base = format!(
        "/prompts/{}/entries/{}",
        field(detail, "day"),
        field(detail, "entryId")
    );
    match detail.get("critiqueId").and_then(Value::as_str) {
        Some(id) => format!("{base}#critique-{id}"),
        None => base,
    }
}

pub const KINDS: &[KindSpec] = &[
    KindSpec {
        kind: "prompt.critique",
        singular: "critique",
        plural: "critiques",
        describe: |actor, d| {
            format!(
                "{actor} critiqued your answer to “{}”",
                field(d, "promptExcerpt")
            )
        },
        link: entry_link,
    },
    KindSpec {
        kind: "prompt.reply",
        singular: "reply",
        plural: "replies",
        describe: |actor, d| {
            if field(d, "role") == "parent" {
                format!(
                    "{actor} replied to your critique on “{}”",
                    field(d, "promptExcerpt")
                )
            } else {
                format!(
                    "{actor} replied in a thread on your answer to “{}”",
                    field(d, "promptExcerpt")
                )
            }
        },
        link: entry_link,
    },
    KindSpec {
        kind: "prompt.report",
        singular: "report",
        plural: "reports",
        describe: |actor, d| {
            format!(
                "{actor} reported a critique on your answer to “{}”",
                field(d, "promptExcerpt")
            )
        },
        link: entry_link,
    },
    // Reviews. `subject` is what is up for review, as the author's book names
    // it: "“The Door” in “Gull Rock”", or the book's title alone.
    KindSpec {
        kind: "review.invited",
        singular: "request to review",
        plural: "requests to review",
        describe: |actor, d| format!("{actor} asked you to review {}", field(d, "subject")),
        link: |d| format!("/review/{}", field(d, "publicationId")),
    },
    KindSpec {
        kind: "review.updated",
        singular: "review brought up to date",
        plural: "reviews brought up to date",
        describe: |actor, d| {
            format!(
                "{actor} put a newer version of {} up for review",
                field(d, "subject")
            )
        },
        link: |d| format!("/review/{}", field(d, "publicationId")),
    },
    // Notices from the site's staff. They never name a moderator.
    KindSpec {
        kind: "moderation.restriction",
        singular: "notice about your account",
        plural: "notices about your account",
        describe: |_, d| field(d, "text").to_string(),
        link: |_| "/standing".into(),
    },
    KindSpec {
        kind: "moderation.restriction_ended",
        singular: "notice about your account",
        plural: "notices about your account",
        describe: |_, d| field(d, "text").to_string(),
        link: |_| "/standing".into(),
    },
    KindSpec {
        kind: "moderation.appeal_decided",
        singular: "decision on your appeal",
        plural: "decisions on your appeals",
        describe: |_, d| field(d, "text").to_string(),
        link: |_| "/standing".into(),
    },
    KindSpec {
        kind: "moderation.content_removed",
        singular: "removed post",
        plural: "removed posts",
        describe: |_, d| field(d, "text").to_string(),
        link: |_| "/standing".into(),
    },
    KindSpec {
        kind: "moderation.book_handed_over",
        singular: "book handed to you",
        plural: "books handed to you",
        describe: |_, d| field(d, "text").to_string(),
        link: |d| format!("/works/{}", field(d, "workId")),
    },
    // Proves the point of the registry: a kind nothing else knows about is
    // listed, counted and summed up by this entry alone.
    #[cfg(test)]
    KindSpec {
        kind: "test.ping",
        singular: "ping",
        plural: "pings",
        describe: |actor, _| format!("{actor} pinged you"),
        link: |_| "/inbox".into(),
    },
];

pub fn spec(kind: &str) -> Option<&'static KindSpec> {
    KINDS.iter().find(|k| k.kind == kind)
}

/// A count as words: "1 critique", "4 replies".
pub fn counted(kind: &str, n: i64) -> String {
    match spec(kind) {
        Some(k) if n == 1 => format!("1 {}", k.singular),
        Some(k) => format!("{n} {}", k.plural),
        None => format!("{n} {kind}"),
    }
}

/// "a, b and c".
pub fn and_list(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The summary's sentence, from its counts in registry order (anything the
/// registry does not know goes last) and the prompts they were about.
pub fn digest_text(counts: &[(String, i64)], prompts: &[String]) -> String {
    let mut ordered: Vec<&(String, i64)> = counts.iter().filter(|(_, n)| *n > 0).collect();
    let rank = |kind: &str| {
        KINDS
            .iter()
            .position(|k| k.kind == kind)
            .unwrap_or(usize::MAX)
    };
    ordered.sort_by_key(|(kind, _)| rank(kind));
    let parts: Vec<String> = ordered.iter().map(|(k, n)| counted(k, *n)).collect();
    if parts.is_empty() {
        return String::new();
    }
    let about = match prompts {
        [one] => format!(" on your answer to “{one}”"),
        [] => String::new(),
        _ => " on your answers".to_string(),
    };
    format!("Yesterday: {}{about}.", and_list(&parts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_kind_describes_itself_and_links_somewhere() {
        let detail = json!({
            "day": "2026-10-12", "entryId": "e1", "critiqueId": "c1",
            "promptExcerpt": "A door", "role": "parent",
            "text": "A notice from the site's staff.", "workId": "w1",
        });
        for k in KINDS {
            assert!(!(k.describe)("Ada", &detail).is_empty(), "{}", k.kind);
            assert!((k.link)(&detail).starts_with('/'), "{}", k.kind);
        }
        assert_eq!(
            (spec("prompt.critique").unwrap().link)(&detail),
            "/prompts/2026-10-12/entries/e1#critique-c1"
        );
    }

    #[test]
    fn the_summary_counts_by_kind() {
        let text = digest_text(
            &[("prompt.reply".into(), 2), ("prompt.critique".into(), 4)],
            &["A door".into()],
        );
        assert_eq!(
            text,
            "Yesterday: 4 critiques and 2 replies on your answer to “A door”."
        );
        assert_eq!(
            digest_text(&[("prompt.critique".into(), 1)], &["a".into(), "b".into()]),
            "Yesterday: 1 critique on your answers."
        );
        assert_eq!(digest_text(&[], &[]), "");
    }

    #[test]
    fn a_new_kind_needs_only_its_registry_entry() {
        assert_eq!(counted("test.ping", 3), "3 pings");
        assert_eq!(
            digest_text(
                &[("test.ping".into(), 1), ("prompt.critique".into(), 2)],
                &[]
            ),
            "Yesterday: 2 critiques and 1 ping."
        );
    }
}
