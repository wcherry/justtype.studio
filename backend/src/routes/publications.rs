//! A publication is the offer of one frozen version to readers. It carries the
//! visibility, the slice of the work on offer, and the prompt the author wants
//! feedback against.
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{comments, publications, users, versions, works};
use crate::domain::enums::{
    ActivityKind, FeedSort, Genre, LengthBand, PublicationScope, Visibility, WindowFilter, WorkKind,
};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::discovery;
use super::support::{self, Page};
use super::versions::{VersionContent, VersionDetail};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatePublicationRequest {
    #[schema(value_type = crate::domain::enums::Visibility)]
    pub visibility: String,
    #[serde(default = "default_scope")]
    #[schema(value_type = crate::domain::enums::PublicationScope)]
    pub scope: String,
    /// Chapter or scene id when `scope` is narrower than the whole work.
    #[serde(default)]
    pub scope_ref_id: Option<String>,
    #[serde(default)]
    pub review_prompt: Option<String>,
    #[serde(default)]
    pub opens_at: Option<String>,
    #[serde(default)]
    pub closes_at: Option<String>,
    /// How many people may comment. Omitted, there is no cap.
    #[serde(default)]
    pub max_reviewers: Option<i32>,
    /// Withhold notes on scenes a reader has not reached. Omitted, it follows
    /// the work: on for a serial, off for anything else.
    #[serde(default)]
    pub spoiler_safe: Option<bool>,
}

fn default_scope() -> String {
    "work".into()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePublicationRequest {
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::Visibility>)]
    pub visibility: Option<String>,
    /// `0` removes the cap.
    #[serde(default)]
    pub max_reviewers: Option<i32>,
    #[serde(default)]
    pub review_prompt: Option<String>,
    #[serde(default)]
    pub closes_at: Option<String>,
    #[serde(default)]
    pub spoiler_safe: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublicationCard {
    #[serde(flatten)]
    pub publication: Publication,
    pub work_title: String,
    pub work_kind: String,
    pub author: support::AuthorCard,
    pub version_label: String,
    pub word_count: i32,
    pub comment_count: i64,
    #[schema(value_type = Option<crate::domain::enums::Genre>)]
    pub genre: Option<String>,
    /// Distinct people who have opened it or left a note, the author excepted.
    pub reader_count: i64,
    /// Other people's publications this author has left notes on — what lifts
    /// their work in the needs-readers queue.
    pub author_reviews_given: i64,
    /// First lines of the published slice, for the feed. Filled in after the
    /// database round trip because it comes off disk — always sent, even when
    /// empty, so clients do not have to treat it as optional.
    pub excerpt: String,
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/publications",
    tag = "publications",
    params(("version_id" = String, Path)),
    request_body = CreatePublicationRequest,
    responses(
        (status = 200, description = "Success", body = PublicationCard),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
    Json(body): Json<CreatePublicationRequest>,
) -> ApiResult<Json<PublicationCard>> {
    if !Visibility::accepts(&body.visibility) {
        return Err(ApiError::BadRequest(format!(
            "visibility must be one of {}",
            Visibility::options()
        )));
    }
    if !PublicationScope::accepts(&body.scope) {
        return Err(ApiError::BadRequest(format!(
            "scope must be one of {}",
            PublicationScope::options()
        )));
    }
    if body.scope != "work" && body.scope_ref_id.is_none() {
        return Err(ApiError::BadRequest(
            "scopeRefId is required when publishing a chapter or a scene".into(),
        ));
    }

    let card = blocking(&state.pool, move |conn| {
        let (version, work) = support::version_for_owner(conn, &version_id, &user)?;
        if version.state == "draft" {
            return Err(ApiError::Conflict(
                "publish the version before offering it for review".into(),
            ));
        }
        let now = now_iso();
        let row = NewPublication {
            id: new_id(),
            work_id: work.id.clone(),
            version_id: version.id.clone(),
            visibility: body.visibility.clone(),
            scope: body.scope.clone(),
            scope_ref_id: body.scope_ref_id.clone(),
            review_prompt: body.review_prompt.clone(),
            opens_at: body.opens_at.clone(),
            closes_at: body.closes_at.clone(),
            created_at: now.clone(),
            updated_at: now,
            max_reviewers: body.max_reviewers,
            // A serial is read as it is written, so one reader is always
            // further in than another. Anything else is finished before anyone
            // opens it, and withholding notes would only hide them.
            spoiler_safe: i32::from(body.spoiler_safe.unwrap_or(work.kind == "serial")),
        };
        diesel::insert_into(publications::table)
            .values(&row)
            .execute(conn)?;
        super::collaborators::record(
            conn,
            &work.id,
            &user.id,
            ActivityKind::PublicationOpened,
            Some(&row.id),
            &format!("{} to {} readers", version.label, row.visibility),
        )?;
        load_card(conn, &row.id)
    })
    .await?;
    let mut card = card;
    attach_excerpt(&state, &mut card).await;
    Ok(Json(card))
}

fn load_card(conn: &mut crate::db::Conn, publication_id: &str) -> ApiResult<PublicationCard> {
    let publication: Publication = publications::table
        .find(publication_id)
        .select(Publication::as_select())
        .first(conn)?;
    let work: Work = works::table
        .find(&publication.work_id)
        .select(Work::as_select())
        .first(conn)?;
    let version: Version = versions::table
        .find(&publication.version_id)
        .select(Version::as_select())
        .first(conn)?;
    let (author_id, display_name): (String, String) = users::table
        .find(&work.owner_id)
        .select((users::id, users::display_name))
        .first(conn)?;
    let comment_count: i64 = comments::table
        .filter(comments::publication_id.eq(publication_id))
        .filter(comments::state.ne("deleted"))
        .count()
        .get_result(conn)?;
    let owners = HashMap::from([(publication.id.clone(), work.owner_id.clone())]);
    let reader_count = discovery::reader_counts(conn, &owners)?
        .remove(&publication.id)
        .unwrap_or(0);
    let author_reviews_given = discovery::reviews_given(conn, std::slice::from_ref(&author_id))?
        .remove(&author_id)
        .unwrap_or(0);

    Ok(PublicationCard {
        genre: work.genre,
        reader_count,
        author_reviews_given,
        work_title: work.title,
        work_kind: work.kind,
        author: support::AuthorCard {
            id: author_id,
            display_name,
        },
        version_label: version.label,
        word_count: version.word_count,
        comment_count,
        publication,
        excerpt: String::new(),
    })
}

/// Reads the opening scene of the published slice off disk and trims it to a
/// card-sized teaser.
async fn attach_excerpt(state: &AppState, card: &mut PublicationCard) {
    let scene_id = match card.publication.scope.as_str() {
        "scene" => card.publication.scope_ref_id.clone(),
        _ => None,
    };
    let first = match scene_id {
        Some(id) => Some(id),
        None => first_scene_id(state, &card.publication.version_id).await,
    };
    if let Some(scene_id) = first {
        if let Ok(content) = state
            .store
            .read_version_scene(
                &card.publication.work_id,
                &card.publication.version_id,
                &scene_id,
            )
            .await
        {
            card.excerpt = crate::domain::doc::excerpt(&content, 280);
        }
    }
}

async fn first_scene_id(state: &AppState, version_id: &str) -> Option<String> {
    let version_id = version_id.to_string();
    crate::db::blocking(&state.pool, move |conn| {
        use crate::db::schema::version_entries as ve;
        Ok(ve::table
            .filter(ve::version_id.eq(&version_id))
            .order((ve::chapter_position.asc(), ve::scene_position.asc()))
            .select(ve::scene_id)
            .first::<String>(conn)
            .optional()?)
    })
    .await
    .ok()
    .flatten()
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedQuery {
    // Deliberately not `#[serde(flatten)] Page`: the urlencoded deserializer
    // behind `Query` does not support flattened structs.
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
    /// `novel`, `short_story`, … to narrow the feed.
    #[serde(default)]
    pub kind: Option<String>,
    /// `true` restricts the feed to the caller's own publications.
    #[serde(default)]
    pub mine: Option<bool>,
    /// One author's publications — what their profile lists.
    #[serde(default)]
    pub author: Option<String>,
    /// `true` restricts the feed to authors the caller follows.
    #[serde(default)]
    pub following: Option<bool>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::Genre>)]
    pub genre: Option<String>,
    /// By the published version's word count.
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::LengthBand>)]
    pub length: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::WindowFilter>)]
    pub window: Option<String>,
    /// `newest` (the default) or `needs_readers`.
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::FeedSort>)]
    pub sort: Option<String>,
}

impl FeedQuery {
    fn validate(&self) -> ApiResult<()> {
        fn one_of(
            name: &str,
            value: &Option<String>,
            accepts: fn(&str) -> bool,
            options: fn() -> String,
        ) -> ApiResult<()> {
            match value {
                Some(value) if !accepts(value) => Err(ApiError::BadRequest(format!(
                    "{name} must be one of {}",
                    options()
                ))),
                _ => Ok(()),
            }
        }
        one_of("kind", &self.kind, WorkKind::accepts, WorkKind::options)?;
        one_of("genre", &self.genre, Genre::accepts, Genre::options)?;
        one_of(
            "length",
            &self.length,
            LengthBand::accepts,
            LengthBand::options,
        )?;
        one_of(
            "window",
            &self.window,
            WindowFilter::accepts,
            WindowFilter::options,
        )?;
        one_of("sort", &self.sort, FeedSort::accepts, FeedSort::options)
    }
}

/// The word counts behind each `LengthBand`, as the awards define them.
fn length_bounds(band: &str) -> (i32, i32) {
    match band {
        "short" => (0, 7_499),
        "novelette" => (7_500, 17_499),
        "novella" => (17_500, 39_999),
        _ => (40_000, i32::MAX),
    }
}

/// How far back the needs-readers queue looks. It is ranked in memory, so it
/// needs a bound; at the scale this runs at, five hundred open publications is
/// every one of them.
const QUEUE_CANDIDATES: i64 = 500;

/// The review feed. Signed-out readers see only `public` publications.
#[utoipa::path(
    get,
    path = "/api/publications",
    tag = "publications",
    responses(
        (status = 200, description = "Success", body = Vec<PublicationCard>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn feed(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Query(query): Query<FeedQuery>,
) -> ApiResult<Json<Vec<PublicationCard>>> {
    Ok(Json(feed_cards(&state, user, query).await?))
}

/// What a caller may be shown, narrowed by the query. Shared with the profile
/// page, which is this list with an author filled in: one rule for what is on
/// offer rather than two that drift.
pub async fn feed_cards(
    state: &AppState,
    user: Option<AuthUser>,
    query: FeedQuery,
) -> ApiResult<Vec<PublicationCard>> {
    query.validate()?;
    let (limit, offset) = Page {
        limit: query.limit.unwrap_or(50),
        offset: query.offset.unwrap_or(0),
    }
    .clamped();
    let me = user.map(|u| (u.id, u.email.trim().to_lowercase()));
    let needs_readers = query.sort.as_deref() == Some("needs_readers");

    let cards = blocking(&state.pool, move |conn| {
        let mut q = publications::table
            .inner_join(works::table.on(works::id.eq(publications::work_id)))
            .inner_join(versions::table.on(versions::id.eq(publications::version_id)))
            .into_boxed();

        match (&me, query.mine.unwrap_or(false)) {
            (Some((id, _)), true) => q = q.filter(works::owner_id.eq(id.clone())),
            (Some((id, email)), false) => {
                use crate::db::schema::{blocked_readers, publication_invites};
                // Named readers have no other way in: there is no mail going
                // out yet, so a draft invisible in the feed is a draft they can
                // only reach if the author sends them the link by hand.
                let invited = publication_invites::table
                    .filter(publication_invites::email.eq(email.clone()))
                    .select(publication_invites::publication_id);
                q = q.filter(
                    publications::visibility
                        .eq_any(vec!["public", "platform"])
                        .or(works::owner_id.eq(id.clone()))
                        .or(publications::id.eq_any(invited)),
                );
                // A window that has closed, or has not opened, refuses the
                // click; listing it would be an invitation to a locked door.
                let now = crate::domain::now_iso();
                q = q.filter(
                    publications::opens_at
                        .is_null()
                        .or(publications::opens_at.le(now.clone()))
                        .or(works::owner_id.eq(id.clone())),
                );
                q = q.filter(
                    publications::closes_at
                        .is_null()
                        .or(publications::closes_at.ge(now))
                        .or(works::owner_id.eq(id.clone())),
                );
                // The same locked door, held shut by the author rather than
                // the calendar.
                let shut_out_by = blocked_readers::table
                    .filter(blocked_readers::user_id.eq(id.clone()))
                    .select(blocked_readers::owner_id);
                q = q.filter(diesel::dsl::not(works::owner_id.eq_any(shut_out_by)));
            }
            (None, _) => q = q.filter(publications::visibility.eq("public")),
        }
        if let Some(kind) = &query.kind {
            q = q.filter(works::kind.eq(kind.clone()));
        }
        if let Some(author) = &query.author {
            q = q.filter(works::owner_id.eq(author.clone()));
        }
        if query.following.unwrap_or(false) {
            use crate::db::schema::follows;
            // Signed out, nobody is followed, and the honest answer is an
            // empty list rather than everyone's.
            let me_id = me.as_ref().map(|(id, _)| id.clone()).unwrap_or_default();
            let followed = follows::table
                .filter(follows::follower_id.eq(me_id))
                .select(follows::followee_id);
            q = q.filter(works::owner_id.eq_any(followed));
        }
        if let Some(genre) = &query.genre {
            q = q.filter(works::genre.eq(genre.clone()));
        }
        if let Some(band) = &query.length {
            let (low, high) = length_bounds(band);
            q = q.filter(versions::word_count.between(low, high));
        }
        match query.window.as_deref() {
            Some("closing_soon") => {
                let now = chrono::Utc::now();
                let soon = (now + chrono::Duration::days(3))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                q = q
                    .filter(publications::closes_at.ge(now_iso()))
                    .filter(publications::closes_at.le(soon));
            }
            Some("open_ended") => q = q.filter(publications::closes_at.is_null()),
            _ => {}
        }

        if !needs_readers {
            let ids: Vec<String> = q
                .order(publications::created_at.desc())
                .limit(limit)
                .offset(offset)
                .select(publications::id)
                .load(conn)?;
            return ids
                .iter()
                .map(|id| load_card(conn, id))
                .collect::<ApiResult<Vec<_>>>();
        }

        // The queue is for finding something to read, so it leaves out the
        // caller's own work and anything they have already opened or remarked
        // on — those have had the reader this caller could give them.
        if let Some((id, _)) = &me {
            use crate::db::schema::reading_progress;
            let opened = reading_progress::table
                .filter(reading_progress::user_id.eq(id.clone()))
                .select(reading_progress::publication_id);
            let remarked = comments::table
                .filter(comments::author_id.eq(id.clone()))
                .select(comments::publication_id);
            q = q
                .filter(works::owner_id.ne(id.clone()))
                .filter(diesel::dsl::not(publications::id.eq_any(opened)))
                .filter(diesel::dsl::not(publications::id.eq_any(remarked)));
        }
        let rows: Vec<(String, String, String)> = q
            .order(publications::created_at.desc())
            .limit(QUEUE_CANDIDATES)
            .select((publications::id, works::owner_id, publications::created_at))
            .load(conn)?;
        let owners: HashMap<String, String> = rows
            .iter()
            .map(|(id, owner, _)| (id.clone(), owner.clone()))
            .collect();
        let readers = discovery::reader_counts(conn, &owners)?;
        let authors: Vec<String> = owners.values().cloned().collect();
        let given = discovery::reviews_given(conn, &authors)?;
        let mut candidates: Vec<discovery::Candidate> = rows
            .into_iter()
            .map(|(id, owner, created_at)| discovery::Candidate {
                readers: readers.get(&id).copied().unwrap_or(0),
                given: given.get(&owner).copied().unwrap_or(0),
                id,
                created_at,
            })
            .collect();
        discovery::rank(&mut candidates);
        let ids: Vec<String> = candidates
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|c| c.id)
            .collect();

        ids.iter()
            .map(|id| load_card(conn, id))
            .collect::<ApiResult<Vec<_>>>()
    })
    .await?;

    let mut cards = cards;
    for card in &mut cards {
        attach_excerpt(state, card).await;
    }
    Ok(cards)
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}",
    tag = "publications",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = PublicationCard),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<PublicationCard>> {
    let card = blocking(&state.pool, move |conn| {
        support::publication_for_reader(conn, &publication_id, user.as_ref())?;
        load_card(conn, &publication_id)
    })
    .await?;
    let mut card = card;
    attach_excerpt(&state, &mut card).await;
    Ok(Json(card))
}

/// The manuscript as reviewers see it: the frozen snapshot, narrowed to the
/// slice the author published.
#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/content",
    tag = "publications",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = VersionContent),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn content(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<VersionContent>> {
    let (publication, detail) = blocking(&state.pool, move |conn| {
        let (publication, _) =
            support::publication_for_reader(conn, &publication_id, user.as_ref())?;
        let version: Version = versions::table
            .find(&publication.version_id)
            .select(Version::as_select())
            .first(conn)?;
        let entries: Vec<VersionEntry> = crate::db::schema::version_entries::table
            .filter(crate::db::schema::version_entries::version_id.eq(&version.id))
            .order((
                crate::db::schema::version_entries::chapter_position.asc(),
                crate::db::schema::version_entries::scene_position.asc(),
            ))
            .select(VersionEntry::as_select())
            .load(conn)?;
        // Tags are the author's own shelf-labels; a reader has no use for them.
        Ok((
            publication,
            VersionDetail {
                version,
                entries,
                tags: Vec::new(),
            },
        ))
    })
    .await?;

    let detail = narrow_to_scope(&publication, detail);
    let content = super::versions::assemble_content(&state, detail).await?;
    Ok(Json(content))
}

fn narrow_to_scope(publication: &Publication, mut detail: VersionDetail) -> VersionDetail {
    match (
        publication.scope.as_str(),
        publication.scope_ref_id.as_deref(),
    ) {
        ("chapter", Some(chapter_id)) => {
            detail.entries.retain(|e| e.chapter_id == chapter_id);
        }
        ("scene", Some(scene_id)) => {
            detail.entries.retain(|e| e.scene_id == scene_id);
        }
        _ => {}
    }
    detail
}

#[utoipa::path(
    patch,
    path = "/api/publications/{publication_id}",
    tag = "publications",
    params(("publication_id" = String, Path)),
    request_body = UpdatePublicationRequest,
    responses(
        (status = 200, description = "Success", body = PublicationCard),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<UpdatePublicationRequest>,
) -> ApiResult<Json<PublicationCard>> {
    if let Some(visibility) = &body.visibility {
        if !Visibility::accepts(visibility) {
            return Err(ApiError::BadRequest(format!(
                "visibility must be one of {}",
                Visibility::options()
            )));
        }
    }
    let card = blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        let now = now_iso();
        if let Some(visibility) = &body.visibility {
            diesel::update(publications::table.find(&publication_id))
                .set(publications::visibility.eq(visibility))
                .execute(conn)?;
        }
        if let Some(prompt) = &body.review_prompt {
            diesel::update(publications::table.find(&publication_id))
                .set(publications::review_prompt.eq(prompt))
                .execute(conn)?;
        }
        if let Some(closes_at) = &body.closes_at {
            diesel::update(publications::table.find(&publication_id))
                .set(publications::closes_at.eq(closes_at))
                .execute(conn)?;
        }
        if let Some(cap) = body.max_reviewers {
            diesel::update(publications::table.find(&publication_id))
                .set(publications::max_reviewers.eq((cap > 0).then_some(cap)))
                .execute(conn)?;
        }
        if let Some(spoiler_safe) = body.spoiler_safe {
            diesel::update(publications::table.find(&publication_id))
                .set(publications::spoiler_safe.eq(i32::from(spoiler_safe)))
                .execute(conn)?;
        }
        diesel::update(publications::table.find(&publication_id))
            .set(publications::updated_at.eq(&now))
            .execute(conn)?;
        load_card(conn, &publication_id)
    })
    .await?;
    Ok(Json(card))
}

/// Withdrawing a publication takes its comments with it; the snapshot itself
/// is untouched.
#[utoipa::path(
    delete,
    path = "/api/publications/{publication_id}",
    tag = "publications",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Success"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_publication(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Value>> {
    blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        diesel::delete(publications::table.find(&publication_id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
