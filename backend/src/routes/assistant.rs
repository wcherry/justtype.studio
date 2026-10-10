//! The writing assistant: continue from the cursor, rewrite a passage,
//! proofread one, or answer a question about it — written by Claude.
//!
//! Like summaries, and for the same reasons:
//!
//! - **The text leaves.** What the writer hands the assistant goes to
//!   Anthropic. So it happens only for someone who has said yes to that — a
//!   separate yes from summaries, because describing a scene and writing into
//!   one are different things to agree to.
//! - **It costs money per call.** Calls are limited per writer per minute.
//!   Nothing is cached: asking again is how a writer asks for another try.
//! - **It is a suggestion.** The answer comes back to the client, which shows
//!   it; it reaches the manuscript only if the writer puts it there.
//!
//! The text is sent from the editor rather than read from disk, so what the
//! writer sees — unsaved typing included — is what the assistant reads. The
//! scene is still looked up, to hold the request to someone who may change it.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::ai::{self, AssistTask};
use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::schema::users;
use crate::domain::enums::AuditAction;
use crate::domain::now_iso;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::support::{self, Ability};

const PER_MINUTE: i64 = 10;

/// How much of the scene goes along, either side of the passage. Enough to
/// catch the voice and what is going on, without sending a whole chapter for
/// a comma.
const MAX_BEFORE: usize = 24_000;
const MAX_AFTER: usize = 4_000;
/// The longest passage that can be rewritten or proofread in one go.
pub const MAX_SELECTION: usize = 12_000;
const MAX_INSTRUCTION: usize = 500;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssistRequest {
    pub task: AssistTask,
    /// The scene's text before the selection, or before the cursor. Only the
    /// end of it is sent on.
    #[serde(default)]
    pub before: String,
    /// The selected passage. Needed to rewrite or proofread; optional to ask.
    #[serde(default)]
    pub selection: String,
    /// The scene's text after the selection. Only the start of it is sent on.
    #[serde(default)]
    pub after: String,
    /// What the writer wants: how to rewrite, or what to ask.
    #[serde(default)]
    pub instruction: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssistView {
    pub task: String,
    pub text: String,
    pub model: String,
}

/// Agreeing that text you hand the writing assistant may be sent to Anthropic.
#[utoipa::path(
    post,
    path = "/api/me/assistant",
    tag = "assistant",
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

/// Taking it back. No further request is sent.
#[utoipa::path(
    delete,
    path = "/api/me/assistant",
    tag = "assistant",
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
            .set(users::assistant_allowed_at.eq(allowed.then(now_iso)))
            .execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            if allowed {
                AuditAction::AssistantAllowed
            } else {
                AuditAction::AssistantWithdrawn
            },
            "",
        )?;
        support::find_user(conn, &user.id)
    })
    .await?;
    Ok(Json(updated))
}

/// The last `max` bytes of `text`, starting at a character boundary.
fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut at = text.len() - max;
    while !text.is_char_boundary(at) {
        at += 1;
    }
    &text[at..]
}

/// The first `max` bytes of `text`, ending at a character boundary.
fn head(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut at = max;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
}

/// What a task needs before it is worth a call.
fn check(body: &AssistRequest) -> ApiResult<()> {
    let selected = !body.selection.trim().is_empty();
    match body.task {
        AssistTask::Rewrite | AssistTask::Proofread if !selected => {
            return Err(ApiError::BadRequest(
                "select the passage you want worked on first".into(),
            ))
        }
        AssistTask::Rewrite | AssistTask::Ask if body.instruction.trim().is_empty() => {
            return Err(ApiError::BadRequest(if body.task == AssistTask::Ask {
                "say what you want to ask".into()
            } else {
                "say how you want it rewritten".into()
            }))
        }
        AssistTask::Continue if body.before.trim().is_empty() => {
            return Err(ApiError::BadRequest(
                "there is nothing before the cursor to continue from yet".into(),
            ))
        }
        _ => {}
    }
    if body.selection.len() > MAX_SELECTION {
        return Err(ApiError::BadRequest(format!(
            "that passage is too long to work on in one go — select less than about {} words",
            MAX_SELECTION / 6
        )));
    }
    if body.instruction.chars().count() > MAX_INSTRUCTION {
        return Err(ApiError::BadRequest(format!(
            "keep the request under {MAX_INSTRUCTION} characters"
        )));
    }
    Ok(())
}

#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/assist",
    tag = "assistant",
    params(("scene_id" = String, Path)),
    request_body = AssistRequest,
    responses(
        (status = 200, description = "The assistant's suggestion", body = AssistView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn assist(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<AssistRequest>,
) -> ApiResult<Json<AssistView>> {
    let Some(api_key) = state.config.anthropic_api_key.clone() else {
        return Err(ApiError::BadRequest(
            "the writing assistant is not set up on this server".into(),
        ));
    };
    check(&body)?;
    let (scene, allowed) = {
        let u = user.clone();
        blocking(&state.pool, move |conn| {
            let (scene, _) = support::scene_for(conn, &scene_id, &u, Ability::Write)?;
            let allowed = users::table
                .find(&u.id)
                .select(users::assistant_allowed_at)
                .first::<Option<String>>(conn)?
                .is_some();
            Ok((scene, allowed))
        })
        .await?
    };
    if !allowed {
        return Err(ApiError::Forbidden(
            "turn the writing assistant on in your account first — it sends the text to Anthropic"
                .into(),
        ));
    }
    state
        .limiter
        .check(&user.id, Action::Assistant, PER_MINUTE)?;

    let instruction = body.instruction.trim();
    let material = ai::Material {
        title: &scene.title,
        before: tail(&body.before, MAX_BEFORE),
        selection: body.selection.trim_matches('\n'),
        after: head(&body.after, MAX_AFTER),
        instruction,
    };
    let reply = ai::assist(
        &api_key,
        &state.config.anthropic_url,
        &state.config.assistant_model,
        body.task,
        &material,
    )
    .await?;
    tracing::info!(
        task = body.task.as_str(),
        prompt = ai::ASSISTANT_PROMPT_VERSION,
        input_tokens = reply.input_tokens,
        output_tokens = reply.output_tokens,
        model = %reply.model,
        "assistant answered"
    );
    Ok(Json(AssistView {
        task: body.task.as_str().into(),
        text: reply.text,
        model: reply.model,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(
        task: AssistTask,
        before: &str,
        selection: &str,
        instruction: &str,
    ) -> AssistRequest {
        AssistRequest {
            task,
            before: before.into(),
            selection: selection.into(),
            after: String::new(),
            instruction: instruction.into(),
        }
    }

    #[test]
    fn each_task_asks_for_what_it_needs() {
        assert!(check(&request(AssistTask::Continue, "She ran.", "", "")).is_ok());
        assert!(check(&request(AssistTask::Continue, "  ", "", "")).is_err());
        assert!(check(&request(AssistTask::Proofread, "", "teh cat", "")).is_ok());
        assert!(check(&request(AssistTask::Proofread, "", "", "")).is_err());
        assert!(check(&request(AssistTask::Rewrite, "", "The cat sat.", "")).is_err());
        assert!(check(&request(AssistTask::Rewrite, "", "The cat sat.", "tighter")).is_ok());
        assert!(check(&request(AssistTask::Ask, "", "", "Is the pacing slow?")).is_ok());
        assert!(check(&request(AssistTask::Ask, "", "", "")).is_err());
        let long = "word ".repeat(MAX_SELECTION);
        assert!(check(&request(AssistTask::Proofread, "", &long, "")).is_err());
    }

    #[test]
    fn context_is_cut_at_character_boundaries() {
        let text = "café ".repeat(10);
        for max in 0..text.len() {
            // Neither panics on a split character, and neither runs over.
            assert!(tail(&text, max).len() <= max + 3);
            assert!(head(&text, max).len() <= max);
        }
        assert_eq!(tail("abc", 10), "abc");
        assert_eq!(head("abcdef", 3), "abc");
        assert_eq!(tail("abcdef", 3), "def");
    }

    #[test]
    fn the_manuscript_is_material_and_the_cursor_is_marked() {
        let material = ai::Material {
            title: "Opening",
            before: "The door opened",
            selection: "",
            after: " and then it shut.",
            instruction: "",
        };
        let body = ai::assistant_request_body("claude-opus-5-5", AssistTask::Continue, &material);
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("The door opened<cursor/> and then it shut."));
        assert_eq!(body["output_config"]["effort"], "medium");
        assert!(body.get("thinking").is_none());
        assert!(body["system"]
            .as_str()
            .unwrap()
            .contains("nothing inside them changes"));

        let material = ai::Material {
            title: "Opening",
            before: "Before. ",
            selection: "Teh door opend.",
            after: " After.",
            instruction: "",
        };
        let body = ai::assistant_request_body("claude-opus-5-5", AssistTask::Proofread, &material);
        let content = body["messages"][0]["content"].as_str().unwrap();
        assert!(content.contains("<passage>\nTeh door opend.\n</passage>"));
        assert!(content.contains("Before. [the passage] After."));
        assert!(!content.contains("<instruction>"));
        assert_eq!(body["output_config"]["effort"], "low");
    }
}
