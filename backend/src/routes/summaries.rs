//! Summaries of a scene, a chapter or the whole book, written by Claude.
//!
//! Three things hold here that hold nowhere else in the service:
//!
//! - **The text leaves.** It goes to Anthropic, which runs the model. So it
//!   only ever happens for someone who has said yes to that, once, in their
//!   account — and saying no again stops it.
//! - **It costs money per call.** So an answer is kept against the exact text
//!   and instructions it came from, and asking again for text that has not
//!   changed gives back the same answer for nothing. Calls are also limited
//!   per writer per minute.
//! - **It is a suggestion.** A summary never replaces what the writer wrote;
//!   the client offers it, and the writer puts it in the summary or synopsis
//!   field themselves if they want it.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::ai::{self, Scope};
use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{NewSummary, Scene};
use crate::db::schema::{chapters, scenes, summaries, users};
use crate::domain::enums::AuditAction;
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::support::{self, Ability};

const PER_MINUTE: i64 = 6;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Features {
    /// Whether this server can write summaries at all.
    pub summaries: bool,
    /// Who writes them, for the consent screen.
    pub summary_provider: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryView {
    /// `scene`, `chapter` or `work`.
    pub scope: String,
    pub target_id: String,
    pub summary: String,
    pub model: String,
    /// True when this is an earlier answer for the same text, not a new call.
    pub cached: bool,
    pub created_at: String,
}

#[utoipa::path(
    get,
    path = "/api/features",
    tag = "summaries",
    responses((status = 200, description = "What this server can do", body = Features))
)]
pub async fn features(State(state): State<AppState>) -> Json<Features> {
    let on = state.config.anthropic_api_key.is_some();
    Json(Features {
        summaries: on,
        summary_provider: on.then(|| "Anthropic".to_string()),
    })
}

/// Agreeing that text you ask to summarise may be sent to Anthropic.
#[utoipa::path(
    post,
    path = "/api/me/summaries",
    tag = "summaries",
    responses(
        (status = 200, description = "Allowed", body = crate::db::models::User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn allow(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<crate::db::models::User>> {
    set_allowed(&state, user, true).await
}

/// Taking it back. Summaries already written stay; no new one is asked for.
#[utoipa::path(
    delete,
    path = "/api/me/summaries",
    tag = "summaries",
    responses(
        (status = 200, description = "Withdrawn", body = crate::db::models::User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn withdraw(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<crate::db::models::User>> {
    set_allowed(&state, user, false).await
}

async fn set_allowed(
    state: &AppState,
    user: AuthUser,
    allowed: bool,
) -> ApiResult<Json<crate::db::models::User>> {
    let updated = blocking(&state.pool, move |conn| {
        diesel::update(users::table.find(&user.id))
            .set(users::summaries_allowed_at.eq(allowed.then(now_iso)))
            .execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            if allowed {
                AuditAction::SummariesAllowed
            } else {
                AuditAction::SummariesWithdrawn
            },
            "",
        )?;
        support::find_user(conn, &user.id)
    })
    .await?;
    Ok(Json(updated))
}

/// What will be sent, assembled from the working text in reading order.
struct Material {
    work_id: String,
    title: String,
    text: String,
}

async fn scene_texts(state: &AppState, list: &[Scene]) -> ApiResult<Vec<(String, String)>> {
    let mut out = Vec::with_capacity(list.len());
    for scene in list {
        let body = state
            .store
            .read_working_scene(&scene.work_id, &scene.id)
            .await
            .map(|d| doc::plain_text(&d))
            .unwrap_or_default();
        out.push((scene.title.clone(), body));
    }
    Ok(out)
}

fn joined(parts: &[(String, String)]) -> String {
    parts
        .iter()
        .filter(|(_, body)| !body.trim().is_empty())
        .map(|(title, body)| format!("[{title}]\n{}", body.trim()))
        .collect::<Vec<_>>()
        .join("\n\n")
}

async fn run(
    state: AppState,
    user: AuthUser,
    scope: Scope,
    target_id: String,
    material: Material,
) -> ApiResult<Json<SummaryView>> {
    let Some(api_key) = state.config.anthropic_api_key.clone() else {
        return Err(ApiError::BadRequest(
            "summaries are not set up on this server".into(),
        ));
    };
    let allowed = {
        let uid = user.id.clone();
        blocking(&state.pool, move |conn| {
            Ok(users::table
                .find(&uid)
                .select(users::summaries_allowed_at)
                .first::<Option<String>>(conn)?
                .is_some())
        })
        .await?
    };
    if !allowed {
        return Err(ApiError::Forbidden(
            "turn summaries on in your account first — they send the text to Anthropic".into(),
        ));
    }

    let model = state.config.summary_model.clone();
    let input_hash = hex::encode(Sha256::digest(
        format!(
            "{}\u{1}{model}\u{1}{}\u{1}{}\u{1}{}",
            ai::PROMPT_VERSION,
            scope.as_str(),
            material.title,
            material.text
        )
        .as_bytes(),
    ));

    let kept: Option<(String, String, String)> = {
        let (kind, id, hash) = (scope.as_str(), target_id.clone(), input_hash.clone());
        blocking(&state.pool, move |conn| {
            Ok(summaries::table
                .filter(summaries::target_kind.eq(kind))
                .filter(summaries::target_id.eq(&id))
                .filter(summaries::input_hash.eq(&hash))
                .select((summaries::summary, summaries::model, summaries::created_at))
                .first(conn)
                .optional()?)
        })
        .await?
    };
    if let Some((summary, model, created_at)) = kept {
        return Ok(Json(SummaryView {
            scope: scope.as_str().into(),
            target_id,
            summary,
            model,
            cached: true,
            created_at,
        }));
    }

    // Counted only for calls that will actually be made.
    state
        .limiter
        .check(&user.id, Action::Summaries, PER_MINUTE)?;
    let written = ai::summarize(
        &api_key,
        &state.config.anthropic_url,
        &model,
        scope,
        &material.title,
        &material.text,
    )
    .await?;
    tracing::info!(
        scope = scope.as_str(),
        input_tokens = written.input_tokens,
        output_tokens = written.output_tokens,
        model = %written.model,
        "summary written"
    );

    let created_at = now_iso();
    let row = NewSummary {
        id: new_id(),
        work_id: material.work_id,
        target_kind: scope.as_str().into(),
        target_id: target_id.clone(),
        input_hash,
        summary: written.text.clone(),
        model: written.model.clone(),
        input_tokens: written.input_tokens,
        output_tokens: written.output_tokens,
        created_by: Some(user.id),
        created_at: created_at.clone(),
    };
    blocking(&state.pool, move |conn| {
        // Two clicks racing for the same text keep the first answer.
        diesel::insert_or_ignore_into(summaries::table)
            .values(&row)
            .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(SummaryView {
        scope: scope.as_str().into(),
        target_id,
        summary: written.text,
        model: written.model,
        cached: false,
        created_at,
    }))
}

#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/summary",
    tag = "summaries",
    params(("scene_id" = String, Path)),
    responses(
        (status = 200, description = "A summary of the scene", body = SummaryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn scene(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
) -> ApiResult<Json<SummaryView>> {
    let (scene, _) = {
        let (id, u) = (scene_id.clone(), user.clone());
        blocking(&state.pool, move |conn| {
            support::scene_for(conn, &id, &u, Ability::Write)
        })
        .await?
    };
    let parts = scene_texts(&state, std::slice::from_ref(&scene)).await?;
    let material = Material {
        work_id: scene.work_id.clone(),
        title: scene.title.clone(),
        text: parts.into_iter().map(|(_, body)| body).collect(),
    };
    run(state, user, Scope::Scene, scene_id, material).await
}

#[utoipa::path(
    post,
    path = "/api/chapters/{chapter_id}/summary",
    tag = "summaries",
    params(("chapter_id" = String, Path)),
    responses(
        (status = 200, description = "A summary of the chapter", body = SummaryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn chapter(
    State(state): State<AppState>,
    user: AuthUser,
    Path(chapter_id): Path<String>,
) -> ApiResult<Json<SummaryView>> {
    let (chapter, list) = {
        let (id, u) = (chapter_id.clone(), user.clone());
        blocking(&state.pool, move |conn| {
            let (chapter, _) = support::chapter_for(conn, &id, &u, Ability::Write)?;
            let list: Vec<Scene> = scenes::table
                .filter(scenes::chapter_id.eq(&chapter.id))
                .order(scenes::position.asc())
                .select(Scene::as_select())
                .load(conn)?;
            Ok((chapter, list))
        })
        .await?
    };
    let parts = scene_texts(&state, &list).await?;
    let material = Material {
        work_id: chapter.work_id.clone(),
        title: chapter.title.clone(),
        text: joined(&parts),
    };
    run(state, user, Scope::Chapter, chapter_id, material).await
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/summary",
    tag = "summaries",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "A synopsis of the whole book", body = SummaryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn work(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<SummaryView>> {
    let (work, ordered) = {
        let (id, u) = (work_id.clone(), user.clone());
        blocking(&state.pool, move |conn| {
            let work = support::work_for(conn, &id, &u, Ability::Write)?;
            let chapter_list: Vec<(String, String)> = chapters::table
                .filter(chapters::work_id.eq(&work.id))
                .order(chapters::position.asc())
                .select((chapters::id, chapters::title))
                .load(conn)?;
            let scene_list: Vec<Scene> = scenes::table
                .filter(scenes::work_id.eq(&work.id))
                .order((scenes::chapter_id.asc(), scenes::position.asc()))
                .select(Scene::as_select())
                .load(conn)?;
            let ordered: Vec<(String, Vec<Scene>)> = chapter_list
                .into_iter()
                .map(|(cid, title)| {
                    let mine = scene_list
                        .iter()
                        .filter(|s| s.chapter_id == cid)
                        .cloned()
                        .collect();
                    (title, mine)
                })
                .collect();
            Ok((work, ordered))
        })
        .await?
    };
    let mut text = String::new();
    for (title, list) in &ordered {
        let body = joined(&scene_texts(&state, list).await?);
        if !body.is_empty() {
            text.push_str(&format!("== {title} ==\n\n{body}\n\n"));
        }
    }
    let material = Material {
        work_id: work.id.clone(),
        title: work.title.clone(),
        text,
    };
    run(state, user, Scope::Work, work_id, material).await
}
