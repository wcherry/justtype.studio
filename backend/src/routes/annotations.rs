//! Highlights and bookmarks: one person's own marks on the text.
//!
//! Nobody else sees them — not the author, not other readers, not a
//! co-author — and they never touch the manuscript. That is why they live
//! here rather than as marks in the document, where a highlight would be
//! printed in the export and shown to every reader of the next version.
//!
//! On a publication the positions are exact: a snapshot does not change. On
//! the working draft they are where the text was when the mark was made; the
//! client finds the quote again when the text has moved, and moves the mark.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{Annotation, NewAnnotation};
use crate::db::schema::{annotations, scenes};
use crate::domain::enums::AnnotationKind;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Ability};

/// Enough to recognise the passage and find it again.
const MAX_QUOTE: usize = 2000;
const MAX_NOTE: usize = 500;
/// Per person per book (or per publication): generous, but not a store.
const MAX_PER_TEXT: i64 = 2000;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationQuery {
    #[serde(default)]
    pub scene_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAnnotationRequest {
    pub scene_id: String,
    #[schema(value_type = AnnotationKind)]
    pub kind: String,
    pub from: i32,
    pub to: i32,
    pub quote: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAnnotationRequest {
    /// Where the passage is now, after the draft moved it.
    #[serde(default)]
    pub from: Option<i32>,
    #[serde(default)]
    pub to: Option<i32>,
    /// The passage as it reads now, when the writer has edited it.
    #[serde(default)]
    pub quote: Option<String>,
    /// Empty clears it.
    #[serde(default)]
    pub note: Option<String>,
}

fn checked(body: &CreateAnnotationRequest) -> ApiResult<(String, Option<String>)> {
    if !AnnotationKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            AnnotationKind::options()
        )));
    }
    check_range(body.from, body.to)?;
    let quote: String = body.quote.trim().chars().take(MAX_QUOTE).collect();
    if quote.is_empty() {
        return Err(ApiError::BadRequest(
            "select some text to mark first".into(),
        ));
    }
    let note = checked_note(body.note.as_deref())?;
    if body.kind == "comment" && note.is_none() {
        return Err(ApiError::BadRequest(
            "a comment needs something to say".into(),
        ));
    }
    Ok((quote, note))
}

fn check_range(from: i32, to: i32) -> ApiResult<()> {
    if from < 0 || to <= from {
        return Err(ApiError::BadRequest(
            "a mark needs a range of text, from before to".into(),
        ));
    }
    Ok(())
}

fn checked_note(note: Option<&str>) -> ApiResult<Option<String>> {
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    match note {
        Some(n) if n.chars().count() > MAX_NOTE => Err(ApiError::BadRequest(format!(
            "a note on a mark runs to {MAX_NOTE} characters at most"
        ))),
        other => Ok(other.map(str::to_string)),
    }
}

fn insert(
    conn: &mut crate::db::Conn,
    user_id: &str,
    work_id: &str,
    publication_id: Option<String>,
    body: CreateAnnotationRequest,
    quote: String,
    note: Option<String>,
) -> ApiResult<Annotation> {
    let mut held = annotations::table
        .filter(annotations::user_id.eq(user_id))
        .filter(annotations::work_id.eq(work_id))
        .into_boxed();
    held = match &publication_id {
        Some(id) => held.filter(annotations::publication_id.eq(id)),
        None => held.filter(annotations::publication_id.is_null()),
    };
    if held.count().get_result::<i64>(conn)? >= MAX_PER_TEXT {
        return Err(ApiError::BadRequest(format!(
            "{MAX_PER_TEXT} marks is the most one book holds — remove some first"
        )));
    }
    let now = now_iso();
    let row = NewAnnotation {
        id: new_id(),
        user_id: user_id.to_string(),
        work_id: work_id.to_string(),
        scene_id: body.scene_id,
        publication_id,
        kind: body.kind,
        from_pos: body.from,
        to_pos: body.to,
        quote,
        note,
        created_at: now.clone(),
        updated_at: now,
    };
    diesel::insert_into(annotations::table)
        .values(&row)
        .execute(conn)?;
    Ok(annotations::table
        .find(&row.id)
        .select(Annotation::as_select())
        .first(conn)?)
}

/// Your marks on the working draft, in reading order within each scene.
#[utoipa::path(
    get,
    path = "/api/works/{work_id}/annotations",
    tag = "annotations",
    params(("work_id" = String, Path), ("sceneId" = Option<String>, Query)),
    responses(
        (status = 200, description = "Yours, and only yours", body = [Annotation]),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_draft(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<AnnotationQuery>,
) -> ApiResult<Json<Vec<Annotation>>> {
    let rows = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        let mut q = annotations::table
            .filter(annotations::user_id.eq(&user.id))
            .filter(annotations::work_id.eq(&work_id))
            .filter(annotations::publication_id.is_null())
            .into_boxed();
        if let Some(scene_id) = &query.scene_id {
            q = q.filter(annotations::scene_id.eq(scene_id));
        }
        Ok(
            q.order((annotations::scene_id.asc(), annotations::from_pos.asc()))
                .select(Annotation::as_select())
                .load(conn)?,
        )
    })
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/annotations",
    tag = "annotations",
    params(("work_id" = String, Path)),
    request_body = CreateAnnotationRequest,
    responses(
        (status = 200, description = "Marked", body = Annotation),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_draft(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateAnnotationRequest>,
) -> ApiResult<Json<Annotation>> {
    let (quote, note) = checked(&body)?;
    let row = blocking(&state.pool, move |conn| {
        // Reading is enough: a mark changes nothing anyone else sees.
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        let in_work: i64 = scenes::table
            .filter(scenes::id.eq(&body.scene_id))
            .filter(scenes::work_id.eq(&work_id))
            .count()
            .get_result(conn)?;
        if in_work == 0 {
            return Err(ApiError::NotFound("scene not found in this book".into()));
        }
        insert(conn, &user.id, &work_id, None, body, quote, note)
    })
    .await?;
    Ok(Json(row))
}

/// Your marks on a published version.
#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/annotations",
    tag = "annotations",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Yours, and only yours", body = [Annotation]),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_publication(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<Annotation>>> {
    let rows = blocking(&state.pool, move |conn| {
        let (publication, _) = support::publication_for_reader(conn, &publication_id, Some(&user))?;
        Ok(annotations::table
            .filter(annotations::user_id.eq(&user.id))
            .filter(annotations::publication_id.eq(&publication.id))
            .order((annotations::scene_id.asc(), annotations::from_pos.asc()))
            .select(Annotation::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/annotations",
    tag = "annotations",
    params(("publication_id" = String, Path)),
    request_body = CreateAnnotationRequest,
    responses(
        (status = 200, description = "Marked", body = Annotation),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_publication(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<CreateAnnotationRequest>,
) -> ApiResult<Json<Annotation>> {
    let (quote, note) = checked(&body)?;
    let row = blocking(&state.pool, move |conn| {
        let (publication, work) =
            support::publication_for_reader(conn, &publication_id, Some(&user))?;
        insert(
            conn,
            &user.id,
            &work.id,
            Some(publication.id),
            body,
            quote,
            note,
        )
    })
    .await?;
    Ok(Json(row))
}

fn own(conn: &mut crate::db::Conn, id: &str, user: &AuthUser) -> ApiResult<Annotation> {
    annotations::table
        .find(id)
        // Someone else's mark is not there at all, as far as you can tell.
        .filter(annotations::user_id.eq(&user.id))
        .select(Annotation::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("mark not found".into()))
}

#[utoipa::path(
    patch,
    path = "/api/annotations/{annotation_id}",
    tag = "annotations",
    params(("annotation_id" = String, Path)),
    request_body = UpdateAnnotationRequest,
    responses(
        (status = 200, description = "Changed", body = Annotation),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(annotation_id): Path<String>,
    Json(body): Json<UpdateAnnotationRequest>,
) -> ApiResult<Json<Annotation>> {
    let note = body
        .note
        .as_deref()
        .map(|n| checked_note(Some(n)))
        .transpose()?;
    let row = blocking(&state.pool, move |conn| {
        let current = own(conn, &annotation_id, &user)?;
        let (from, to) = (
            body.from.unwrap_or(current.from_pos),
            body.to.unwrap_or(current.to_pos),
        );
        let quote = match body.quote.as_deref().map(str::trim) {
            Some("") => {
                return Err(ApiError::BadRequest("a mark needs some text".into()));
            }
            Some(q) => q.chars().take(MAX_QUOTE).collect(),
            None => current.quote.clone(),
        };
        if (from, to) != (current.from_pos, current.to_pos) || quote != current.quote {
            // A snapshot does not move, so neither does a mark on one.
            if current.publication_id.is_some() {
                return Err(ApiError::BadRequest(
                    "a mark on a published version stays where it is".into(),
                ));
            }
            check_range(from, to)?;
        }
        diesel::update(annotations::table.find(&current.id))
            .set((
                annotations::from_pos.eq(from),
                annotations::to_pos.eq(to),
                annotations::quote.eq(quote),
                annotations::note.eq(note.unwrap_or(current.note)),
                annotations::updated_at.eq(now_iso()),
            ))
            .execute(conn)?;
        own(conn, &current.id, &user)
    })
    .await?;
    Ok(Json(row))
}

#[utoipa::path(
    delete,
    path = "/api/annotations/{annotation_id}",
    tag = "annotations",
    params(("annotation_id" = String, Path)),
    responses(
        (status = 204, description = "Removed"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_annotation(
    State(state): State<AppState>,
    user: AuthUser,
    Path(annotation_id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    blocking(&state.pool, move |conn| {
        let current = own(conn, &annotation_id, &user)?;
        diesel::delete(annotations::table.find(&current.id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(kind: &str, from: i32, to: i32, quote: &str) -> CreateAnnotationRequest {
        CreateAnnotationRequest {
            scene_id: "s".into(),
            kind: kind.into(),
            from,
            to,
            quote: quote.into(),
            note: None,
        }
    }

    #[test]
    fn a_mark_needs_a_kind_a_range_and_some_text() {
        assert!(checked(&request("highlight", 1, 5, "the field")).is_ok());
        assert!(checked(&request("bookmark", 1, 5, "the field")).is_ok());
        assert!(checked(&request("underline", 1, 5, "the field")).is_err());
        assert!(checked(&request("highlight", 5, 5, "the field")).is_err());
        assert!(checked(&request("highlight", -1, 5, "the field")).is_err());
        assert!(checked(&request("highlight", 1, 5, "   ")).is_err());
        assert_eq!(checked_note(Some("  ")).unwrap(), None);
        assert!(checked_note(Some(&"x".repeat(MAX_NOTE + 1))).is_err());
    }
}
