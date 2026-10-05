//! Who has read what, and who has read for whom.
//!
//! The "needs readers" queue is the feed in a different order: least-read
//! first, with reciprocity lifting the work of people who read for others. Both
//! numbers are derived on read from rows that already exist — a progress row or
//! a note is what "read" means here — rather than counters kept beside them.
use std::collections::{HashMap, HashSet};

use diesel::prelude::*;

use crate::db::schema::{comments, publications, reading_progress, works};
use crate::db::Conn;
use crate::error::ApiResult;

/// How many other people's publications count towards an author's placement.
///
/// Capped so that reciprocity is a nudge rather than a currency: past ten,
/// someone who reviews compulsively should not be able to bury everyone who
/// has only just arrived.
pub const RECIPROCITY_CAP: i64 = 10;

/// Distinct people who have read or remarked on each publication, the author
/// excepted. `owners` maps publication id to its author.
pub fn reader_counts(
    conn: &mut Conn,
    owners: &HashMap<String, String>,
) -> ApiResult<HashMap<String, i64>> {
    let ids: Vec<&String> = owners.keys().collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut readers: HashMap<String, HashSet<String>> = HashMap::new();
    let progress: Vec<(String, String)> = reading_progress::table
        .filter(reading_progress::publication_id.eq_any(&ids))
        .select((reading_progress::publication_id, reading_progress::user_id))
        .load(conn)?;
    let notes: Vec<(String, String)> = comments::table
        .filter(comments::publication_id.eq_any(&ids))
        .filter(comments::state.ne("deleted"))
        .select((comments::publication_id, comments::author_id))
        .load(conn)?;
    for (publication_id, user_id) in progress.into_iter().chain(notes) {
        if owners.get(&publication_id) == Some(&user_id) {
            continue;
        }
        readers.entry(publication_id).or_default().insert(user_id);
    }
    Ok(owners
        .keys()
        .map(|id| (id.clone(), readers.get(id).map_or(0, |r| r.len() as i64)))
        .collect())
}

/// How many of other people's publications each author has left notes on.
///
/// A publication counts once however many notes went on it — twenty typo
/// reports on one chapter are one act of reading, not twenty.
pub fn reviews_given(conn: &mut Conn, authors: &[String]) -> ApiResult<HashMap<String, i64>> {
    if authors.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, String)> = comments::table
        .inner_join(publications::table.on(publications::id.eq(comments::publication_id)))
        .inner_join(works::table.on(works::id.eq(publications::work_id)))
        .filter(comments::author_id.eq_any(authors))
        .filter(comments::state.ne("deleted"))
        .filter(works::owner_id.ne(comments::author_id))
        .select((comments::author_id, publications::id))
        .distinct()
        .load(conn)?;
    let mut given: HashMap<String, i64> = HashMap::new();
    for (author, _) in rows {
        *given.entry(author).or_default() += 1;
    }
    Ok(given)
}

/// Where a publication sits in the queue: higher goes first.
///
/// Every reader it already has costs it a place, and every piece of someone
/// else's work its author has read earns one back, up to the cap. A newcomer
/// with nothing read and nothing given sits level with someone who has given
/// as much as they got, and both sit above anyone who has had readers without
/// returning the favour.
pub fn placement(readers: i64, given: i64) -> i64 {
    given.min(RECIPROCITY_CAP) - readers
}

/// Orders candidates for the queue. Ties go to fewer readers, then to whoever
/// has been waiting longest.
pub fn rank(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        placement(b.readers, b.given)
            .cmp(&placement(a.readers, a.given))
            .then(a.readers.cmp(&b.readers))
            .then(a.created_at.cmp(&b.created_at))
    });
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: String,
    pub created_at: String,
    pub readers: i64,
    pub given: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, created_at: &str, readers: i64, given: i64) -> Candidate {
        Candidate {
            id: id.into(),
            created_at: created_at.into(),
            readers,
            given,
        }
    }

    fn order(mut candidates: Vec<Candidate>) -> Vec<String> {
        rank(&mut candidates);
        candidates.into_iter().map(|c| c.id).collect()
    }

    #[test]
    fn the_least_read_comes_first() {
        assert_eq!(
            order(vec![
                candidate("read-twice", "2026-01-01", 2, 0),
                candidate("unread", "2026-01-02", 0, 0),
                candidate("read-once", "2026-01-03", 1, 0),
            ]),
            ["unread", "read-once", "read-twice"]
        );
    }

    #[test]
    fn reading_for_others_lifts_your_own_work() {
        // Same readers each; the one whose author has read for three other
        // people goes ahead of the one whose author has read for nobody.
        assert_eq!(
            order(vec![
                candidate("taker", "2026-01-01", 1, 0),
                candidate("giver", "2026-01-02", 1, 3),
            ]),
            ["giver", "taker"]
        );
        // And it can outweigh having had a reader already.
        assert_eq!(
            order(vec![
                candidate("unread-taker", "2026-01-01", 0, 0),
                candidate("read-giver", "2026-01-02", 1, 4),
            ]),
            ["read-giver", "unread-taker"]
        );
    }

    #[test]
    fn reciprocity_is_capped() {
        assert_eq!(placement(0, 500), RECIPROCITY_CAP);
        // Past the cap, readers decide again.
        assert_eq!(
            order(vec![
                candidate("prolific", "2026-01-01", 3, 400),
                candidate("generous", "2026-01-02", 1, RECIPROCITY_CAP),
            ]),
            ["generous", "prolific"]
        );
    }

    #[test]
    fn ties_go_to_whoever_has_waited_longest() {
        assert_eq!(
            order(vec![
                candidate("newer", "2026-03-01", 0, 0),
                candidate("older", "2026-01-01", 0, 0),
            ]),
            ["older", "newer"]
        );
    }
}
