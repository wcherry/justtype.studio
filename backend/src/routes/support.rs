//! Lookups that also enforce access. Every handler that touches a work goes
//! through one of these, so authorisation lives in one file instead of being
//! re-derived in twenty handlers.
use diesel::prelude::*;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::Conn;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

impl Default for Page {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
        }
    }
}

impl Page {
    pub fn clamped(&self) -> (i64, i64) {
        (self.limit.clamp(1, 200), self.offset.max(0))
    }
}

pub fn find_user(conn: &mut Conn, user_id: &str) -> ApiResult<User> {
    users::table
        .find(user_id)
        .select(User::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("user not found".into()))
}

/// A work the caller owns. Collaborator roles land here when sharing ships.
pub fn work_for_owner(conn: &mut Conn, work_id: &str, user: &AuthUser) -> ApiResult<Work> {
    let work: Work = works::table
        .find(work_id)
        .select(Work::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("work not found".into()))?;
    if work.owner_id != user.id {
        return Err(ApiError::Forbidden(
            "you do not have access to this work".into(),
        ));
    }
    Ok(work)
}

pub fn chapter_for_owner(
    conn: &mut Conn,
    chapter_id: &str,
    user: &AuthUser,
) -> ApiResult<(Chapter, Work)> {
    let chapter: Chapter = chapters::table
        .find(chapter_id)
        .select(Chapter::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("chapter not found".into()))?;
    let work = work_for_owner(conn, &chapter.work_id, user)?;
    Ok((chapter, work))
}

pub fn scene_for_owner(
    conn: &mut Conn,
    scene_id: &str,
    user: &AuthUser,
) -> ApiResult<(Scene, Work)> {
    let scene: Scene = scenes::table
        .find(scene_id)
        .select(Scene::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("scene not found".into()))?;
    let work = work_for_owner(conn, &scene.work_id, user)?;
    Ok((scene, work))
}

pub fn version_for_owner(
    conn: &mut Conn,
    version_id: &str,
    user: &AuthUser,
) -> ApiResult<(Version, Work)> {
    let version: Version = versions::table
        .find(version_id)
        .select(Version::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("version not found".into()))?;
    let work = work_for_owner(conn, &version.work_id, user)?;
    Ok((version, work))
}

pub fn find_publication(conn: &mut Conn, publication_id: &str) -> ApiResult<Publication> {
    publications::table
        .find(publication_id)
        .select(Publication::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("publication not found".into()))
}

/// Reader-side access.
///
/// `public` is open to all, `unlisted` is link-only, `platform` wants anyone
/// signed in, and `invited` wants the reader's address on the invitation list.
/// The author always gets through their own door.
///
/// The review window is checked here rather than at each caller: a publication
/// that has not opened, or has closed, is not readable by anyone but its
/// author, and forgetting that check in one handler would quietly leak a
/// manuscript.
pub fn publication_for_reader(
    conn: &mut Conn,
    publication_id: &str,
    user: Option<&AuthUser>,
) -> ApiResult<(Publication, Work)> {
    let publication = find_publication(conn, publication_id)?;
    let work: Work = works::table
        .find(&publication.work_id)
        .select(Work::as_select())
        .first(conn)?;
    if user.map(|u| u.id == work.owner_id).unwrap_or(false) {
        return Ok((publication, work));
    }

    let now = crate::domain::now_iso();
    if publication
        .opens_at
        .as_deref()
        .is_some_and(|at| now.as_str() < at)
    {
        return Err(ApiError::Forbidden("this review has not opened yet".into()));
    }
    if publication
        .closes_at
        .as_deref()
        .is_some_and(|at| at < now.as_str())
    {
        return Err(ApiError::Forbidden("this review window has closed".into()));
    }

    match publication.visibility.as_str() {
        "public" | "unlisted" => Ok((publication, work)),
        "platform" if user.is_some() => Ok((publication, work)),
        "invited" => {
            let Some(user) = user else {
                return Err(ApiError::Forbidden(
                    "this manuscript was shared with named readers — sign in to open it".into(),
                ));
            };
            if invited(conn, &publication.id, &user.email)? {
                Ok((publication, work))
            } else {
                Err(ApiError::Forbidden(
                    "this manuscript was shared with named readers, and yours is not among them"
                        .into(),
                ))
            }
        }
        _ => Err(ApiError::Forbidden(
            "sign in to read this publication".into(),
        )),
    }
}

/// Whether an address is on a publication's invitation list.
pub fn invited(conn: &mut Conn, publication_id: &str, email: &str) -> ApiResult<bool> {
    use crate::db::schema::publication_invites;
    let count: i64 = publication_invites::table
        .filter(publication_invites::publication_id.eq(publication_id))
        .filter(publication_invites::email.eq(email.trim().to_lowercase()))
        .count()
        .get_result(conn)?;
    Ok(count > 0)
}

/// Records that an invited reader turned up, which is how an author can tell
/// who actually opened the thing.
pub fn mark_invite_accepted(
    conn: &mut Conn,
    publication_id: &str,
    user: &AuthUser,
) -> ApiResult<()> {
    use crate::db::schema::publication_invites;
    diesel::update(
        publication_invites::table
            .filter(publication_invites::publication_id.eq(publication_id))
            .filter(publication_invites::email.eq(user.email.trim().to_lowercase()))
            .filter(publication_invites::accepted_at.is_null()),
    )
    .set((
        publication_invites::accepted_at.eq(crate::domain::now_iso()),
        publication_invites::accepted_by.eq(&user.id),
    ))
    .execute(conn)?;
    Ok(())
}

/// Refuses a would-be reviewer once the cap is full.
///
/// Counted by distinct people who have already commented, so the cap limits
/// voices rather than comments — someone who has said one thing may say
/// another.
pub fn within_reviewer_cap(
    conn: &mut Conn,
    publication: &Publication,
    user: &AuthUser,
) -> ApiResult<()> {
    let Some(cap) = publication.max_reviewers else {
        return Ok(());
    };
    use crate::db::schema::comments;
    let existing: Vec<String> = comments::table
        .filter(comments::publication_id.eq(&publication.id))
        .filter(comments::state.ne("deleted"))
        .select(comments::author_id)
        .distinct()
        .load(conn)?;
    if existing.iter().any(|id| id == &user.id) || (existing.len() as i32) < cap {
        return Ok(());
    }
    Err(ApiError::Forbidden(format!(
        "this review is full — the author asked for {cap} reader{}",
        if cap == 1 { "" } else { "s" }
    )))
}

/// Is this scene actually part of what was published?
///
/// The snapshot answers "does this scene exist"; the publication's scope
/// answers "was it on offer". Feedback pointed at a scene outside the slice is
/// feedback on something the reader was never shown.
pub fn scene_in_publication(
    conn: &mut Conn,
    publication: &Publication,
    scene_id: &str,
) -> ApiResult<()> {
    use crate::db::schema::version_entries;
    let entry: Option<(String, String)> = version_entries::table
        .filter(version_entries::version_id.eq(&publication.version_id))
        .filter(version_entries::scene_id.eq(scene_id))
        .select((version_entries::chapter_id, version_entries::scene_id))
        .first(conn)
        .optional()?;
    let Some((chapter_id, scene_id)) = entry else {
        return Err(ApiError::BadRequest(
            "that scene is not in this publication".into(),
        ));
    };
    let within = match (
        publication.scope.as_str(),
        publication.scope_ref_id.as_deref(),
    ) {
        ("chapter", Some(wanted)) => chapter_id == wanted,
        ("scene", Some(wanted)) => scene_id == wanted,
        _ => true,
    };
    if within {
        Ok(())
    } else {
        Err(ApiError::BadRequest(
            "that scene is not in this publication".into(),
        ))
    }
}

pub fn publication_for_owner(
    conn: &mut Conn,
    publication_id: &str,
    user: &AuthUser,
) -> ApiResult<(Publication, Work)> {
    let publication = find_publication(conn, publication_id)?;
    let work = work_for_owner(conn, &publication.work_id, user)?;
    Ok((publication, work))
}

/// Keeps `works.word_count` honest after any scene write.
pub fn recount_work(conn: &mut Conn, work_id: &str, now: &str) -> ApiResult<i32> {
    let total: i64 = scenes::table
        .filter(scenes::work_id.eq(work_id))
        .select(diesel::dsl::sum(scenes::word_count))
        .first::<Option<i64>>(conn)?
        .unwrap_or(0);
    let total = total as i32;
    diesel::update(works::table.find(work_id))
        .set((works::word_count.eq(total), works::updated_at.eq(now)))
        .execute(conn)?;
    Ok(total)
}

pub fn next_position(conn: &mut Conn, chapter_id: &str) -> ApiResult<i32> {
    let max: Option<i32> = scenes::table
        .filter(scenes::chapter_id.eq(chapter_id))
        .select(diesel::dsl::max(scenes::position))
        .first(conn)?;
    Ok(max.unwrap_or(-1) + 1)
}

pub fn next_chapter_position(conn: &mut Conn, work_id: &str) -> ApiResult<i32> {
    let max: Option<i32> = chapters::table
        .filter(chapters::work_id.eq(work_id))
        .select(diesel::dsl::max(chapters::position))
        .first(conn)?;
    Ok(max.unwrap_or(-1) + 1)
}

/// An author acting on a note is a stronger signal than a bystander agreeing
/// with it, so a resolve counts for more than an upvote. The number is a
/// judgement, not a measurement; it lives here so there is one of it.
const RESOLVE_WEIGHT: i64 = 3;

/// How useful a reviewer's feedback has turned out to be.
///
/// Derived on read rather than kept in a column: it is a function of rows that
/// already exist, and a stored copy would only be a second thing to get wrong.
#[derive(Debug, Clone, Default, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub comments: i64,
    /// Notes a work's author marked resolved — them saying they acted on it.
    pub resolved: i64,
    /// Net votes across everything this reviewer has written.
    pub votes: i64,
    /// `votes + 3 x resolved`.
    pub score: i64,
}

pub fn standings(
    conn: &mut Conn,
    ids: &[String],
) -> ApiResult<std::collections::HashMap<String, Standing>> {
    use crate::db::schema::comments;
    let mut out: std::collections::HashMap<String, Standing> = Default::default();
    if ids.is_empty() {
        return Ok(out);
    }
    let rows: Vec<(String, String, i32)> = comments::table
        .filter(comments::author_id.eq_any(ids))
        .filter(comments::state.ne("deleted"))
        .select((comments::author_id, comments::state, comments::score))
        .load(conn)?;
    for (author_id, state, score) in rows {
        let entry = out.entry(author_id).or_default();
        entry.comments += 1;
        entry.votes += i64::from(score);
        if state == "resolved" {
            entry.resolved += 1;
        }
    }
    for standing in out.values_mut() {
        standing.score = standing.votes + RESOLVE_WEIGHT * standing.resolved;
    }
    Ok(out)
}

/// Author cards attached to comments so the clients need no second round trip.
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthorCard {
    pub id: String,
    pub display_name: String,
}

pub fn author_cards(
    conn: &mut Conn,
    ids: &[String],
) -> ApiResult<std::collections::HashMap<String, AuthorCard>> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let rows: Vec<(String, String)> = users::table
        .filter(users::id.eq_any(ids))
        .select((users::id, users::display_name))
        .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|(id, display_name)| (id.clone(), AuthorCard { id, display_name }))
        .collect())
}
