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

#[derive(Debug, Deserialize)]
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

/// Reader-side access: anyone signed in can read a `platform` publication, an
/// `unlisted` one is link-only (same rule for now), `public` is open to all.
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
    let is_owner = user.map(|u| u.id == work.owner_id).unwrap_or(false);
    if is_owner {
        return Ok((publication, work));
    }
    match publication.visibility.as_str() {
        "public" | "unlisted" => Ok((publication, work)),
        "platform" if user.is_some() => Ok((publication, work)),
        _ => Err(ApiError::Forbidden(
            "sign in to read this publication".into(),
        )),
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

/// Author cards attached to comments so the clients need no second round trip.
#[derive(Debug, Clone, serde::Serialize)]
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
