//! Full-text search: within one work's working text, and across what is on
//! offer in the review feed.
//!
//! Both indexes are brought up to date by the search that reads them — see the
//! migration for why. A refresh compares what each indexed row was built from
//! with what is there now and re-reads only what moved, so the cost of a
//! search after one edit is one file.
use std::collections::{HashMap, HashSet};

use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::AuthUser;
use crate::db::schema::{publications, scenes, users, version_entries, works};
use crate::db::{blocking, Conn};
use crate::domain::doc;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Ability};

/// Marks a matched word in a snippet. Control characters rather than HTML, so
/// a client never has to decide whether to trust markup that came from a
/// manuscript.
pub const MARK_START: char = '\u{2}';
pub const MARK_END: char = '\u{3}';

fn scene_sql() -> String {
    format!(
        "SELECT scene_id, snippet(scene_search, -1, '{MARK_START}', '{MARK_END}', '…', 14) AS snippet \
         FROM scene_search WHERE scene_search MATCH ? AND work_id = ? \
         ORDER BY bm25(scene_search, 0.0, 0.0, 4.0, 1.0) LIMIT 50"
    )
}

/// The short fields: title outweighs author, which outweighs the prompt and
/// the synopsis.
fn publication_sql() -> String {
    format!(
        "SELECT publication_id, snippet(publication_search, -1, '{MARK_START}', '{MARK_END}', '…', 14) AS snippet \
         FROM publication_search WHERE publication_search MATCH ? \
         ORDER BY bm25(publication_search, 0.0, 10.0, 6.0, 3.0, 3.0) LIMIT ?"
    )
}

/// The published text, searched second — see the migration for why apart.
fn publication_text_sql() -> String {
    format!(
        "SELECT publication_id, snippet(publication_text, 1, '{MARK_START}', '{MARK_END}', '…', 14) AS snippet \
         FROM publication_text WHERE publication_text MATCH ? \
         ORDER BY bm25(publication_text) LIMIT ?"
    )
}

/// A snippet can span paragraphs, and even scenes; it is shown as one line.
fn one_line(snippet: String) -> String {
    snippet.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Turns what a person typed into an FTS5 query that cannot be a syntax error:
/// every word quoted, so `AND`, `NEAR`, a stray quote or a colon mean nothing
/// special, and the last word matches as a prefix because that is what
/// search-as-you-type needs.
pub fn fts_query(input: &str) -> Option<String> {
    let words: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'' || *c == '-')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .take(12)
        .collect();
    if words.is_empty() {
        return None;
    }
    let last = words.len() - 1;
    Some(
        words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let quoted = format!("\"{}\"", w.replace('"', "\"\""));
                if i == last {
                    format!("{quoted}*")
                } else {
                    quoted
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

// --- a work's own text ---------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub q: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SceneHit {
    pub scene_id: String,
    pub chapter_id: String,
    pub title: String,
    /// A few words around the match; matched words sit between U+0002 and
    /// U+0003.
    pub snippet: String,
}

#[derive(QueryableByName)]
struct HitRow {
    #[diesel(sql_type = Text)]
    scene_id: String,
    #[diesel(sql_type = Text)]
    snippet: String,
}

async fn refresh_work(state: &AppState, work_id: &str) -> ApiResult<()> {
    let wid = work_id.to_string();
    let stale: Vec<(String, String, String)> = blocking(&state.pool, move |conn| {
        let current: Vec<(String, String, String)> = scenes::table
            .filter(scenes::work_id.eq(&wid))
            .select((scenes::id, scenes::title, scenes::content_hash))
            .load(conn)?;
        let indexed: HashMap<String, (String, String)> = diesel::sql_query(
            "SELECT scene_id, content_hash, title FROM scene_search_state WHERE work_id = ?",
        )
        .bind::<Text, _>(&wid)
        .load::<StateRow>(conn)?
        .into_iter()
        .map(|r| (r.scene_id, (r.content_hash, r.title)))
        .collect();
        let live: HashSet<&String> = current.iter().map(|(id, _, _)| id).collect();
        // Scenes that have gone since the last search.
        for gone in indexed.keys().filter(|id| !live.contains(id)) {
            forget_scene(conn, gone)?;
        }
        Ok(current
            .into_iter()
            .filter(|(id, title, hash)| indexed.get(id) != Some(&(hash.clone(), title.clone())))
            .collect())
    })
    .await?;
    if stale.is_empty() {
        return Ok(());
    }

    let mut bodies = Vec::with_capacity(stale.len());
    for (scene_id, title, hash) in stale {
        let text = state
            .store
            .read_working_scene(work_id, &scene_id)
            .await
            .map(|d| doc::plain_text(&d))
            .unwrap_or_default();
        bodies.push((scene_id, title, hash, text));
    }
    let wid = work_id.to_string();
    blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            for (scene_id, title, hash, text) in &bodies {
                forget_scene(conn, scene_id)?;
                diesel::sql_query(
                    "INSERT INTO scene_search (scene_id, work_id, title, body) VALUES (?, ?, ?, ?)",
                )
                .bind::<Text, _>(scene_id)
                .bind::<Text, _>(&wid)
                .bind::<Text, _>(title)
                .bind::<Text, _>(text)
                .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO scene_search_state (scene_id, work_id, content_hash, title) VALUES (?, ?, ?, ?)",
                )
                .bind::<Text, _>(scene_id)
                .bind::<Text, _>(&wid)
                .bind::<Text, _>(hash)
                .bind::<Text, _>(title)
                .execute(conn)?;
            }
            Ok(())
        })
    })
    .await
}

#[derive(QueryableByName)]
struct StateRow {
    #[diesel(sql_type = Text)]
    scene_id: String,
    #[diesel(sql_type = Text)]
    content_hash: String,
    #[diesel(sql_type = Text)]
    title: String,
}

fn forget_scene(conn: &mut Conn, scene_id: &str) -> ApiResult<()> {
    diesel::sql_query("DELETE FROM scene_search WHERE scene_id = ?")
        .bind::<Text, _>(scene_id)
        .execute(conn)?;
    diesel::sql_query("DELETE FROM scene_search_state WHERE scene_id = ?")
        .bind::<Text, _>(scene_id)
        .execute(conn)?;
    Ok(())
}

/// Finds words in a work's working text, best matches first.
#[utoipa::path(
    get,
    path = "/api/works/{work_id}/search",
    tag = "works",
    params(("work_id" = String, Path), ("q" = String, Query)),
    responses(
        (status = 200, description = "Scenes that match", body = Vec<SceneHit>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn search_work(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<SceneHit>>> {
    let Some(fts) = fts_query(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    {
        let (wid, user) = (work_id.clone(), user.clone());
        blocking(&state.pool, move |conn| {
            support::work_for(conn, &wid, &user, Ability::Read)?;
            Ok(())
        })
        .await?;
    }
    refresh_work(&state, &work_id).await?;
    let hits = blocking(&state.pool, move |conn| {
        let rows: Vec<HitRow> = diesel::sql_query(scene_sql())
            .bind::<Text, _>(&fts)
            .bind::<Text, _>(&work_id)
            .load(conn)?;
        let ids: Vec<String> = rows.iter().map(|r| r.scene_id.clone()).collect();
        let found: HashMap<String, (String, String)> = scenes::table
            .filter(scenes::id.eq_any(&ids))
            .select((scenes::id, scenes::chapter_id, scenes::title))
            .load::<(String, String, String)>(conn)?
            .into_iter()
            .map(|(id, chapter, title)| (id, (chapter, title)))
            .collect();
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                found
                    .get(&row.scene_id)
                    .map(|(chapter_id, title)| SceneHit {
                        scene_id: row.scene_id.clone(),
                        chapter_id: chapter_id.clone(),
                        title: title.clone(),
                        snippet: one_line(row.snippet),
                    })
            })
            .collect())
    })
    .await?;
    Ok(Json(hits))
}

// --- what is on offer ----------------------------------------------------

#[derive(QueryableByName)]
struct SignatureRow {
    #[diesel(sql_type = Text)]
    publication_id: String,
    #[diesel(sql_type = Text)]
    signature: String,
}

#[derive(QueryableByName)]
struct RankRow {
    #[diesel(sql_type = Text)]
    publication_id: String,
    #[diesel(sql_type = Text)]
    snippet: String,
}

/// Brings the feed's index up to date with every publication.
async fn refresh_publications(state: &AppState) -> ApiResult<()> {
    type Meta = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    );
    let (changed, gone): (Vec<(Meta, String)>, Vec<String>) = blocking(&state.pool, |conn| {
        let rows: Vec<Meta> = publications::table
            .inner_join(works::table.on(works::id.eq(publications::work_id)))
            .inner_join(users::table.on(users::id.eq(works::owner_id)))
            .select((
                publications::id,
                publications::version_id,
                works::title,
                users::display_name,
                publications::review_prompt,
                works::synopsis,
                publications::scope,
                works::id,
            ))
            .load(conn)?;
        let indexed: HashMap<String, String> =
            diesel::sql_query("SELECT publication_id, signature FROM publication_search_state")
                .load::<SignatureRow>(conn)?
                .into_iter()
                .map(|r| (r.publication_id, r.signature))
                .collect();
        let live: HashSet<&String> = rows.iter().map(|r| &r.0).collect();
        let gone: Vec<String> = indexed
            .keys()
            .filter(|id| !live.contains(id))
            .cloned()
            .collect();
        let changed = rows
            .into_iter()
            .filter_map(|row| {
                let signature = hex::encode(Sha256::digest(
                    format!(
                        "{}\u{1}{}\u{1}{}\u{1}{}",
                        row.2,
                        row.3,
                        row.4.as_deref().unwrap_or_default(),
                        row.5.as_deref().unwrap_or_default()
                    )
                    .as_bytes(),
                ));
                (indexed.get(&row.0) != Some(&signature)).then_some((row, signature))
            })
            .collect();
        Ok((changed, gone))
    })
    .await?;

    if changed.is_empty() && gone.is_empty() {
        return Ok(());
    }

    // The published text, for the ones that changed: the snapshot, narrowed
    // to the publication's slice, read through the same cache readers use.
    let mut prepared = Vec::with_capacity(changed.len());
    for ((id, version_id, title, author, prompt, synopsis, _scope, work_id), signature) in changed {
        let pid = id.clone();
        let scenes: Vec<String> = blocking(&state.pool, move |conn| {
            let publication = support::find_publication(conn, &pid)?;
            let entries: Vec<(String, String)> = version_entries::table
                .filter(version_entries::version_id.eq(&publication.version_id))
                .order((
                    version_entries::chapter_position.asc(),
                    version_entries::scene_position.asc(),
                ))
                .select((version_entries::chapter_id, version_entries::scene_id))
                .load(conn)?;
            Ok(entries
                .into_iter()
                .filter(|(chapter, scene)| {
                    match (
                        publication.scope.as_str(),
                        publication.scope_ref_id.as_deref(),
                    ) {
                        ("chapter", Some(want)) => chapter == want,
                        ("scene", Some(want)) => scene == want,
                        _ => true,
                    }
                })
                .map(|(_, scene)| scene)
                .collect())
        })
        .await?;
        let mut body = String::new();
        for scene_id in scenes {
            let doc = match state.scene_cache.get(&version_id, &scene_id) {
                Some(d) => (*d).clone(),
                None => match state
                    .store
                    .read_version_scene(&work_id, &version_id, &scene_id)
                    .await
                {
                    Ok(d) => (*state.scene_cache.put(&version_id, &scene_id, d)).clone(),
                    Err(_) => continue,
                },
            };
            body.push_str(&doc::plain_text(&doc));
            body.push('\n');
        }
        prepared.push((
            id,
            title,
            author,
            prompt.unwrap_or_default(),
            synopsis.unwrap_or_default(),
            body,
            signature,
        ));
    }

    blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            for id in gone.iter().chain(prepared.iter().map(|p| &p.0)) {
                diesel::sql_query("DELETE FROM publication_search WHERE publication_id = ?")
                    .bind::<Text, _>(id)
                    .execute(conn)?;
                diesel::sql_query("DELETE FROM publication_text WHERE publication_id = ?")
                    .bind::<Text, _>(id)
                    .execute(conn)?;
                diesel::sql_query("DELETE FROM publication_search_state WHERE publication_id = ?")
                    .bind::<Text, _>(id)
                    .execute(conn)?;
            }
            for (id, title, author, prompt, synopsis, body, signature) in &prepared {
                diesel::sql_query(
                    "INSERT INTO publication_search (publication_id, title, author, prompt, synopsis) \
                     VALUES (?, ?, ?, ?, ?)",
                )
                .bind::<Text, _>(id)
                .bind::<Text, _>(title)
                .bind::<Text, _>(author)
                .bind::<Text, _>(prompt)
                .bind::<Text, _>(synopsis)
                .execute(conn)?;
                diesel::sql_query("INSERT INTO publication_text (publication_id, body) VALUES (?, ?)")
                    .bind::<Text, _>(id)
                    .bind::<Text, _>(body)
                    .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO publication_search_state (publication_id, signature) VALUES (?, ?)",
                )
                .bind::<Text, _>(id)
                .bind::<Text, _>(signature)
                .execute(conn)?;
            }
            Ok(())
        })
    })
    .await
}

/// Publications matching a query, best first, each with a snippet. The feed
/// then applies its usual rules to these, so a search never reaches anything
/// the feed would not have shown.
pub async fn rank_publications(
    state: &AppState,
    q: &str,
) -> ApiResult<Option<Vec<(String, String)>>> {
    let Some(fts) = fts_query(q) else {
        return Ok(None);
    };
    refresh_publications(state).await?;
    let rows = blocking(&state.pool, move |conn| {
        // Title and author count for most; a word in the text counts, but a
        // novel has a great many words.
        let mut rows = diesel::sql_query(publication_sql())
            .bind::<Text, _>(&fts)
            .bind::<BigInt, _>(500_i64)
            .load::<RankRow>(conn)?;
        // Then the text, for whatever the short fields did not already find.
        let seen: HashSet<String> = rows.iter().map(|r| r.publication_id.clone()).collect();
        rows.extend(
            diesel::sql_query(publication_text_sql())
                .bind::<Text, _>(&fts)
                .bind::<BigInt, _>(500_i64)
                .load::<RankRow>(conn)?
                .into_iter()
                .filter(|r| !seen.contains(&r.publication_id)),
        );
        Ok(rows)
    })
    .await?;
    Ok(Some(
        rows.into_iter()
            .map(|r| (r.publication_id, one_line(r.snippet)))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whatever_is_typed_becomes_a_query_that_cannot_fail() {
        assert_eq!(fts_query("light house").unwrap(), r#""light" "house"*"#);
        assert_eq!(
            fts_query(r#"say "NEAR" AND: o'clock"#).unwrap(),
            r#""say" "NEAR" "AND" "o'clock"*"#
        );
        assert_eq!(fts_query("   "), None);
        assert_eq!(fts_query("***"), None);
    }

    /// FTS5 is a compile-time option of SQLite; this fails loudly if the
    /// bundled build ever stops including it.
    #[test]
    fn the_bundled_sqlite_has_fts5_and_the_queries_run() {
        let dir = std::env::temp_dir().join(format!("scriptorium-fts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = crate::db::build_pool(dir.join("t.db").to_str().unwrap()).unwrap();
        crate::db::run_migrations(&pool).unwrap();
        let mut conn = pool.get().unwrap();
        diesel::sql_query(
            "INSERT INTO scene_search (scene_id, work_id, title, body) VALUES \
             ('s1', 'w', 'The Lighthouse', 'The keepers walked out along the causeway.'), \
             ('s2', 'w', 'Harbour', 'Nothing about light here, only the lighthouses far off.')",
        )
        .execute(&mut conn)
        .unwrap();
        let rows: Vec<HitRow> = diesel::sql_query(scene_sql())
            .bind::<Text, _>(fts_query("lighthouse").unwrap())
            .bind::<Text, _>("w")
            .load(&mut conn)
            .unwrap();
        // A title match outranks a word in passing; stemming finds the plural.
        assert_eq!(
            rows.iter().map(|r| r.scene_id.as_str()).collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        assert!(rows[0].snippet.contains(MARK_START));

        diesel::sql_query(
            "INSERT INTO publication_search (publication_id, title, author, prompt, synopsis) VALUES \
             ('p1', 'Low Tide', 'Sam', '', ''), ('p2', 'Other', 'Tide Writer', '', ''), \
             ('p3', 'Field', 'Ada', '', ''), ('p4', 'Hill', 'Bo', '', ''), ('p5', 'Wood', 'Cy', '', '')",
        )
        .execute(&mut conn)
        .unwrap();
        let ranked: Vec<RankRow> = diesel::sql_query(publication_sql())
            .bind::<Text, _>(fts_query("tide").unwrap())
            .bind::<BigInt, _>(10_i64)
            .load(&mut conn)
            .unwrap();
        // The title outweighs the author's name. (A corpus where most
        // documents do not match, as a real feed is: with every document
        // matching, BM25's rarity term vanishes and weights stop mattering.)
        assert_eq!(
            ranked
                .iter()
                .map(|r| r.publication_id.as_str())
                .collect::<Vec<_>>(),
            ["p1", "p2"]
        );
        diesel::sql_query(
            "INSERT INTO publication_text (publication_id, body) VALUES \
             ('p1', 'The water went out\n\nfurther than anyone remembered.')",
        )
        .execute(&mut conn)
        .unwrap();
        let text: Vec<RankRow> = diesel::sql_query(publication_text_sql())
            .bind::<Text, _>(fts_query("remember").unwrap())
            .bind::<BigInt, _>(10_i64)
            .load(&mut conn)
            .unwrap();
        assert_eq!(text.len(), 1);
        assert!(one_line(text[0].snippet.clone()).contains("further than"));
        assert!(!one_line("a\n\n b".into()).contains('\n'));
        drop(conn);
        std::fs::remove_dir_all(&dir).ok();
    }
}
