//! Lookups that also enforce access. Every handler that touches a work goes
//! through one of these, so authorisation lives in one file instead of being
//! re-derived in twenty handlers.
use std::collections::HashSet;

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

/// What someone may do with a work. Each includes the ones before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ability {
    /// Open the working draft, its history and its snapshots.
    Read,
    /// Propose changes and keep the to-do list, without changing the text.
    Suggest,
    /// Change the text and the structure.
    Write,
    /// Everything else: snapshots, publications, front matter, who is on it.
    Own,
}

/// A caller's standing on one work.
#[derive(Debug, Clone)]
pub struct Access {
    /// `owner`, `writer`, `editor` or `reader`. The creator is an `owner`.
    pub role: String,
    /// The chapters they may change, when they were handed only some. `None`
    /// is the whole work. Reading is never narrowed: a ghostwriter on chapter
    /// nine still needs the eight before it.
    pub chapters: Option<HashSet<String>>,
    /// The person who started the work, as opposed to an owner they added.
    pub creator: bool,
}

impl Access {
    pub fn ability(&self) -> Ability {
        match self.role.as_str() {
            "owner" => Ability::Own,
            "writer" => Ability::Write,
            "editor" => Ability::Suggest,
            _ => Ability::Read,
        }
    }

    pub fn can(&self, need: Ability) -> bool {
        self.ability() >= need
    }

    fn require(&self, need: Ability) -> ApiResult<()> {
        if self.can(need) {
            return Ok(());
        }
        Err(ApiError::Forbidden(match (self.role.as_str(), need) {
            ("reader", _) => "you were added to this work as a reader".into(),
            ("editor", Ability::Write) => {
                "editors propose changes rather than making them — suggest it instead".into()
            }
            _ => "only an owner of this work can do that".into(),
        }))
    }

    /// For changes aimed at one chapter. Reading is never narrowed.
    fn require_in(&self, need: Ability, chapter_id: &str) -> ApiResult<()> {
        self.require(need)?;
        match &self.chapters {
            Some(chapters) if need > Ability::Read && !chapters.contains(chapter_id) => Err(
                ApiError::Forbidden("that chapter is not one of yours on this work".into()),
            ),
            _ => Ok(()),
        }
    }

    /// For changes to the shape of the whole work — adding, ordering or
    /// removing chapters — which someone handed only some chapters cannot make.
    fn require_whole(&self, need: Ability) -> ApiResult<()> {
        self.require(need)?;
        if need > Ability::Read && self.chapters.is_some() {
            return Err(ApiError::Forbidden(
                "you were handed some chapters of this work, not its structure".into(),
            ));
        }
        Ok(())
    }
}

/// Who the caller is to this work, if anyone.
///
/// A collaborator is found by address — the invitation went to an email, and
/// may have gone out before they had an account — and the first time they turn
/// up, their account is recorded against it.
pub fn access(conn: &mut Conn, work: &Work, user: &AuthUser) -> ApiResult<Option<Access>> {
    use crate::db::schema::{collaborator_chapters, work_collaborators};
    if work.owner_id == user.id {
        return Ok(Some(Access {
            role: "owner".into(),
            chapters: None,
            creator: true,
        }));
    }
    // A seat on a book is addressed to an email, so it is only theirs once
    // they have proved the email is.
    if !user.email_verified {
        return Ok(None);
    }
    let email = user.email.trim().to_lowercase();
    let Some(row) = work_collaborators::table
        .filter(work_collaborators::work_id.eq(&work.id))
        .filter(work_collaborators::email.eq(&email))
        .select(WorkCollaborator::as_select())
        .first(conn)
        .optional()?
    else {
        return Ok(None);
    };
    // Turning up is what accepts it — even for someone whose account was
    // known when they were added, and so has an id but has not opened it.
    if row.accepted_at.is_none() {
        diesel::update(work_collaborators::table.find(&row.id))
            .set((
                work_collaborators::user_id.eq(&user.id),
                work_collaborators::accepted_at.eq(crate::domain::now_iso()),
            ))
            .execute(conn)?;
    }
    let chapters = if row.chapter_scoped != 0 {
        Some(
            collaborator_chapters::table
                .filter(collaborator_chapters::collaborator_id.eq(&row.id))
                .select(collaborator_chapters::chapter_id)
                .load::<String>(conn)?
                .into_iter()
                .collect(),
        )
    } else {
        None
    };
    Ok(Some(Access {
        role: row.role,
        chapters,
        creator: false,
    }))
}

fn find_work(conn: &mut Conn, work_id: &str) -> ApiResult<Work> {
    works::table
        .find(work_id)
        .select(Work::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("work not found".into()))
}

/// A work and the caller's standing on it; refused if they have none.
pub fn work_access(conn: &mut Conn, work_id: &str, user: &AuthUser) -> ApiResult<(Work, Access)> {
    let work = find_work(conn, work_id)?;
    match access(conn, &work, user)? {
        Some(access) => Ok((work, access)),
        None if !user.email_verified => Err(ApiError::Forbidden(
            "confirm your email address first — work shared with it opens once you have".into(),
        )),
        None => Err(ApiError::Forbidden(
            "you do not have access to this work".into(),
        )),
    }
}

/// A work the caller may act on as `need` asks, anywhere in it that is not a
/// particular chapter — the notebook, the to-do list, the snapshots.
pub fn work_for(conn: &mut Conn, work_id: &str, user: &AuthUser, need: Ability) -> ApiResult<Work> {
    let (work, access) = work_access(conn, work_id, user)?;
    access.require(need)?;
    Ok(work)
}

/// A work whose structure the caller may change.
pub fn whole_work_for(
    conn: &mut Conn,
    work_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<Work> {
    let (work, access) = work_access(conn, work_id, user)?;
    access.require_whole(need)?;
    Ok(work)
}

/// A work the caller owns — the creator, or an owner they added.
pub fn work_for_owner(conn: &mut Conn, work_id: &str, user: &AuthUser) -> ApiResult<Work> {
    work_for(conn, work_id, user, Ability::Own)
}

/// Whether the caller is the author as far as readers are concerned: they see
/// every reader's reactions and answers rather than only their own.
pub fn is_author(conn: &mut Conn, work: &Work, user: &AuthUser) -> ApiResult<bool> {
    Ok(access(conn, work, user)?.is_some_and(|a| a.can(Ability::Own)))
}

pub fn chapter_for(
    conn: &mut Conn,
    chapter_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<(Chapter, Work)> {
    let chapter: Chapter = chapters::table
        .find(chapter_id)
        .select(Chapter::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("chapter not found".into()))?;
    let (work, access) = work_access(conn, &chapter.work_id, user)?;
    access.require_in(need, &chapter.id)?;
    Ok((chapter, work))
}

pub fn scene_for(
    conn: &mut Conn,
    scene_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<(Scene, Work)> {
    let scene: Scene = scenes::table
        .find(scene_id)
        .select(Scene::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("scene not found".into()))?;
    let (work, access) = work_access(conn, &scene.work_id, user)?;
    access.require_in(need, &scene.chapter_id)?;
    Ok((scene, work))
}

pub fn version_for(
    conn: &mut Conn,
    version_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<(Version, Work)> {
    let version: Version = versions::table
        .find(version_id)
        .select(Version::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("version not found".into()))?;
    let work = work_for(conn, &version.work_id, user, need)?;
    Ok((version, work))
}

pub fn version_for_owner(
    conn: &mut Conn,
    version_id: &str,
    user: &AuthUser,
) -> ApiResult<(Version, Work)> {
    version_for(conn, version_id, user, Ability::Own)
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
    // The people writing it can always open it, whatever the window says.
    if let Some(user) = user {
        if access(conn, &work, user)?.is_some() {
            return Ok((publication, work));
        }
    }

    // An account that is deactivated or permanently banned is not on the site:
    // its publications are gone from every public read.
    if crate::domain::restrictions::hidden_users(conn)?.contains(&work.owner_id) {
        return Err(ApiError::NotFound("publication not found".into()));
    }

    // A block is one writer's door, not a platform ban, and it is checked here
    // so that it covers everything a reader could do with a publication rather
    // than only the handlers someone remembered to guard.
    if let Some(user) = user {
        if blocked(conn, &work.owner_id, &user.id)? {
            return Err(ApiError::Forbidden(
                "this author has asked that you not read their work".into(),
            ));
        }
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
        "public" => Ok((publication, work)),
        // Opened by a signed, expiring link the author made, not by its id.
        "unlisted" if super::share::presented_for(&publication) => Ok((publication, work)),
        "unlisted" => Err(ApiError::Forbidden(
            "this draft opens from the link its author sent — that link is missing, expired or withdrawn"
                .into(),
        )),
        "platform" if user.is_some() => Ok((publication, work)),
        "invited" => {
            let Some(user) = user else {
                return Err(ApiError::Forbidden(
                    "this manuscript was shared with named readers — sign in to open it".into(),
                ));
            };
            // An invitation names an address; until the account has proved
            // that address, anyone could have typed it in at sign-up.
            if !user.email_verified {
                return Err(ApiError::Forbidden(
                    "confirm your email address first — drafts shared with it open once you have"
                        .into(),
                ));
            }
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
    if !user.email_verified {
        return Ok(());
    }
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

/// The scenes a reader has actually got to, for a spoiler-safe publication.
///
/// "Reached" means at or before their furthest point, counted in the
/// publication's own reading order — the same index `reading_progress` keeps,
/// so the two can never disagree about where someone is.
pub fn scenes_reached(
    conn: &mut Conn,
    publication: &Publication,
    user: &AuthUser,
) -> ApiResult<std::collections::HashSet<String>> {
    use crate::db::schema::{reading_progress, version_entries};
    let order: Vec<String> = version_entries::table
        .filter(version_entries::version_id.eq(&publication.version_id))
        .order((
            version_entries::chapter_position.asc(),
            version_entries::scene_position.asc(),
        ))
        .select(version_entries::scene_id)
        .load(conn)?;
    let furthest: Option<i32> = reading_progress::table
        .filter(reading_progress::publication_id.eq(&publication.id))
        .filter(reading_progress::user_id.eq(&user.id))
        .select(reading_progress::furthest_index)
        .first(conn)
        .optional()?;
    // Someone who has not opened it yet has reached the first scene: they are
    // about to read it, and a reader who sees nothing assumes it is broken.
    let reached = furthest.unwrap_or(0).max(0) as usize;
    Ok(order.into_iter().take(reached + 1).collect())
}

/// Has this author shut their door on this reader?
pub fn blocked(conn: &mut Conn, owner_id: &str, user_id: &str) -> ApiResult<bool> {
    use crate::db::schema::blocked_readers;
    let count: i64 = blocked_readers::table
        .filter(blocked_readers::owner_id.eq(owner_id))
        .filter(blocked_readers::user_id.eq(user_id))
        .count()
        .get_result(conn)?;
    Ok(count > 0)
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

/// What to call a scene in an activity line or a message: its title, or
/// "Scene 2" for where it sits in its chapter when it has none.
pub fn scene_label(conn: &mut Conn, scene: &Scene) -> QueryResult<String> {
    if !scene.title.trim().is_empty() {
        return Ok(scene.title.trim().to_string());
    }
    let order: Vec<String> = scenes::table
        .filter(scenes::chapter_id.eq(&scene.chapter_id))
        .order(scenes::position.asc())
        .select(scenes::id)
        .load(conn)?;
    let number = order
        .iter()
        .position(|id| id == &scene.id)
        .map_or(order.len() + 1, |i| i + 1);
    Ok(crate::domain::naming::scene_name(&scene.title, number))
}

/// What to call a chapter: its title, or "Chapter 3" for where it sits.
pub fn chapter_label(conn: &mut Conn, chapter: &Chapter) -> QueryResult<String> {
    if !chapter.title.trim().is_empty() {
        return Ok(chapter.title.trim().to_string());
    }
    let order: Vec<String> = chapters::table
        .filter(chapters::work_id.eq(&chapter.work_id))
        .order(chapters::position.asc())
        .select(chapters::id)
        .load(conn)?;
    let number = order
        .iter()
        .position(|id| id == &chapter.id)
        .map_or(order.len() + 1, |i| i + 1);
    Ok(crate::domain::naming::chapter_name(&chapter.title, number))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(role: &str, chapters: Option<&[&str]>) -> Access {
        Access {
            role: role.into(),
            chapters: chapters.map(|c| c.iter().map(|s| s.to_string()).collect()),
            creator: false,
        }
    }

    #[test]
    fn each_role_includes_the_ones_below_it() {
        let ladder = [
            ("reader", Ability::Read),
            ("editor", Ability::Suggest),
            ("writer", Ability::Write),
            ("owner", Ability::Own),
        ];
        for (role, top) in ladder {
            let a = access(role, None);
            for need in [
                Ability::Read,
                Ability::Suggest,
                Ability::Write,
                Ability::Own,
            ] {
                assert_eq!(a.can(need), need <= top, "{role} and {need:?}");
            }
        }
        // Anything unrecognised reads and does nothing else.
        assert!(!access("ghost", None).can(Ability::Suggest));
    }

    #[test]
    fn a_chapter_grant_narrows_changes_but_never_reading() {
        let ghost = access("writer", Some(&["nine"]));
        assert!(ghost.require_in(Ability::Write, "nine").is_ok());
        assert!(ghost.require_in(Ability::Write, "eight").is_err());
        assert!(ghost.require_in(Ability::Suggest, "eight").is_err());
        // The eight chapters before it are what makes chapter nine writable.
        assert!(ghost.require_in(Ability::Read, "eight").is_ok());
    }

    #[test]
    fn an_empty_grant_is_nowhere_not_everywhere() {
        let stranded = access("writer", Some(&[]));
        assert!(stranded.require_in(Ability::Write, "one").is_err());
        assert!(stranded.require_in(Ability::Read, "one").is_ok());
    }

    #[test]
    fn only_the_whole_work_may_change_its_shape() {
        assert!(access("writer", None).require_whole(Ability::Write).is_ok());
        assert!(access("writer", Some(&["nine"]))
            .require_whole(Ability::Write)
            .is_err());
        assert!(access("editor", None)
            .require_whole(Ability::Write)
            .is_err());
    }
}
