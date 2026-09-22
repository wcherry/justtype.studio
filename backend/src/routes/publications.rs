//! A publication is the offer of one frozen version to readers. It carries the
//! visibility, the slice of the work on offer, and the prompt the author wants
//! feedback against.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{comments, publications, users, versions, works};
use crate::domain::enums::{PublicationScope, Visibility};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

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
    #[serde(default)]
    pub review_prompt: Option<String>,
    #[serde(default)]
    pub closes_at: Option<String>,
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
        };
        diesel::insert_into(publications::table)
            .values(&row)
            .execute(conn)?;
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

    Ok(PublicationCard {
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
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
}

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
    let (limit, offset) = Page {
        limit: query.limit.unwrap_or(50),
        offset: query.offset.unwrap_or(0),
    }
    .clamped();
    let user_id = user.map(|u| u.id);

    let cards = blocking(&state.pool, move |conn| {
        let mut q = publications::table
            .inner_join(works::table.on(works::id.eq(publications::work_id)))
            .into_boxed();

        match (&user_id, query.mine.unwrap_or(false)) {
            (Some(id), true) => q = q.filter(works::owner_id.eq(id.clone())),
            (Some(id), false) => {
                q = q.filter(
                    publications::visibility
                        .eq_any(vec!["public", "platform"])
                        .or(works::owner_id.eq(id.clone())),
                )
            }
            (None, _) => q = q.filter(publications::visibility.eq("public")),
        }
        if let Some(kind) = &query.kind {
            q = q.filter(works::kind.eq(kind.clone()));
        }

        let ids: Vec<String> = q
            .order(publications::created_at.desc())
            .limit(limit)
            .offset(offset)
            .select(publications::id)
            .load(conn)?;

        ids.iter()
            .map(|id| load_card(conn, id))
            .collect::<ApiResult<Vec<_>>>()
    })
    .await?;

    let mut cards = cards;
    for card in &mut cards {
        attach_excerpt(&state, card).await;
    }
    Ok(Json(cards))
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
        Ok((publication, VersionDetail { version, entries }))
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
