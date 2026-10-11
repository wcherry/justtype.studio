//! The feedback a reader will actually give.
//!
//! Most readers will not write a paragraph about your paragraph. They will tell
//! you in one tap that they got lost here and loved that, and they will answer
//! a question you asked them directly. Both are cheaper than a comment, and
//! both say things a comment does not: where attention broke, and how a chapter
//! landed for everyone rather than for whoever felt like writing.
use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{reactions, review_answers, review_questions};
use crate::domain::enums::{QuestionKind, QuestionScope, ReactionKind};
use crate::domain::filter::Surface;
use crate::domain::moderation;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::support::{self, AuthorCard};

// --- reactions -----------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReactRequest {
    pub scene_id: String,
    /// Index of the paragraph in the snapshot scene.
    pub block_index: i32,
    /// Sending the kind already there clears it, the way a repeated vote does.
    #[schema(value_type = crate::domain::enums::ReactionKind)]
    pub kind: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReactionCount {
    #[schema(value_type = crate::domain::enums::ReactionKind)]
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockReactions {
    pub scene_id: String,
    pub block_index: i32,
    /// The opening of the paragraph, so the author's list reads as prose
    /// rather than as coordinates. Empty if the block has since gone.
    pub quote: String,
    /// Totals, for the author only — see the note on `list_reactions`.
    pub counts: Vec<ReactionCount>,
    /// What the viewer themselves marked, if anything.
    #[schema(value_type = Option<crate::domain::enums::ReactionKind>)]
    pub mine: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/reactions",
    tag = "feedback",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Where readers reacted", body = Vec<BlockReactions>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
/// A reader sees only their own marks; the author sees the totals.
///
/// Not a policy about privacy — it is about not corrupting the signal. A
/// reader who can see that four people found this paragraph confusing is being
/// told what to think about it, and the fifth mark stops meaning anything.
pub async fn list_reactions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<BlockReactions>>> {
    let (publication, is_author, rows) = blocking(&state.pool, {
        let publication_id = publication_id.clone();
        let user = user.clone();
        move |conn| {
            let (publication, work) =
                support::publication_for_reader(conn, &publication_id, Some(&user))?;
            let is_author = support::is_author(conn, &work, &user)?;
            let mut q = reactions::table
                .filter(reactions::publication_id.eq(&publication.id))
                .into_boxed();
            if !is_author {
                q = q.filter(reactions::user_id.eq(&user.id));
            }
            let rows: Vec<Reaction> = q
                .order((reactions::scene_id.asc(), reactions::block_index.asc()))
                .select(Reaction::as_select())
                .load(conn)?;
            Ok((publication, is_author, rows))
        }
    })
    .await?;

    let mut blocks = group_reactions(rows, &user.id, is_author);
    attach_quotes(&state, &publication, &mut blocks).await;
    Ok(Json(blocks))
}

#[utoipa::path(
    put,
    path = "/api/publications/{publication_id}/reactions",
    tag = "feedback",
    params(("publication_id" = String, Path)),
    request_body = ReactRequest,
    responses(
        (status = 200, description = "The block as the viewer now sees it", body = BlockReactions),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn react(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<ReactRequest>,
) -> ApiResult<Json<BlockReactions>> {
    if !ReactionKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            ReactionKind::options()
        )));
    }
    if body.block_index < 0 {
        return Err(ApiError::BadRequest("blockIndex cannot be negative".into()));
    }
    state
        .limiter
        .check(&user.id, Action::React, state.config.reaction_rate)?;

    let (scene_id, block_index) = (body.scene_id.clone(), body.block_index);

    // Who may react, and to what. The paragraph is checked against the snapshot
    // rather than trusted: a reaction on a paragraph that is not there is not a
    // signal, it is a row nobody can ever explain.
    let (publication, is_author) = blocking(&state.pool, {
        let publication_id = publication_id.clone();
        let user = user.clone();
        let scene_id = scene_id.clone();
        move |conn| {
            let (publication, work) =
                support::publication_for_reader(conn, &publication_id, Some(&user))?;
            let is_author = support::is_author(conn, &work, &user)?;
            if !is_author {
                support::within_reviewer_cap(conn, &publication, &user)?;
                support::mark_invite_accepted(conn, &publication.id, &user)?;
            }
            support::scene_in_publication(conn, &publication, &scene_id)?;
            Ok((publication, is_author))
        }
    })
    .await?;

    let doc = state
        .store
        .read_version_scene(&publication.work_id, &publication.version_id, &scene_id)
        .await?;
    if block_index as usize >= crate::domain::doc::block_count(&doc) {
        return Err(ApiError::BadRequest(
            "that paragraph is not in this scene".into(),
        ));
    }

    let rows = blocking(&state.pool, {
        let publication = publication.clone();
        let user = user.clone();
        move |conn| {
            let mine: Option<Reaction> = reactions::table
                .filter(reactions::publication_id.eq(&publication.id))
                .filter(reactions::user_id.eq(&user.id))
                .filter(reactions::scene_id.eq(&body.scene_id))
                .filter(reactions::block_index.eq(body.block_index))
                .select(Reaction::as_select())
                .first(conn)
                .optional()?;

            match mine {
                // The same face twice means "no, take that back".
                Some(row) if row.kind == body.kind => {
                    diesel::delete(reactions::table.find(&row.id)).execute(conn)?;
                }
                Some(row) => {
                    diesel::update(reactions::table.find(&row.id))
                        .set(reactions::kind.eq(&body.kind))
                        .execute(conn)?;
                }
                None => {
                    diesel::insert_into(reactions::table)
                        .values(&NewReaction {
                            id: new_id(),
                            publication_id: publication.id.clone(),
                            user_id: user.id.clone(),
                            scene_id: body.scene_id.clone(),
                            block_index: body.block_index,
                            kind: body.kind.clone(),
                            created_at: now_iso(),
                        })
                        .execute(conn)?;
                }
            }

            let mut q = reactions::table
                .filter(reactions::publication_id.eq(&publication.id))
                .filter(reactions::scene_id.eq(&body.scene_id))
                .filter(reactions::block_index.eq(body.block_index))
                .into_boxed();
            if !is_author {
                q = q.filter(reactions::user_id.eq(&user.id));
            }
            Ok(q.select(Reaction::as_select()).load(conn)?)
        }
    })
    .await?;

    let mut blocks = group_reactions(rows, &user.id, is_author);
    if blocks.is_empty() {
        // The mark was cleared. The client still needs the row back, so it can
        // render the block as unmarked rather than guess.
        blocks.push(BlockReactions {
            scene_id,
            block_index,
            quote: String::new(),
            counts: Vec::new(),
            mine: None,
        });
    }
    attach_quotes(&state, &publication, &mut blocks).await;
    Ok(Json(blocks.remove(0)))
}

/// Rows in, one entry per block out, with the counts already added up.
fn group_reactions(rows: Vec<Reaction>, viewer_id: &str, counts: bool) -> Vec<BlockReactions> {
    /// Per block: how many of each kind, and what the viewer marked.
    type Tally = (HashMap<String, i64>, Option<String>);
    let mut order: Vec<(String, i32)> = Vec::new();
    let mut tally: HashMap<(String, i32), Tally> = HashMap::new();
    for row in rows {
        let key = (row.scene_id.clone(), row.block_index);
        let entry = tally.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            (HashMap::new(), None)
        });
        *entry.0.entry(row.kind.clone()).or_insert(0) += 1;
        if row.user_id == viewer_id {
            entry.1 = Some(row.kind);
        }
    }

    order
        .into_iter()
        .map(|key| {
            let (kinds, mine) = tally.remove(&key).unwrap_or_default();
            BlockReactions {
                scene_id: key.0,
                block_index: key.1,
                quote: String::new(),
                // Declared order rather than count order, so a heat map does
                // not reshuffle its own legend as votes come in.
                counts: if counts {
                    ReactionKind::ALL
                        .iter()
                        .filter_map(|kind| {
                            kinds.get(*kind).map(|count| ReactionCount {
                                kind: (*kind).to_string(),
                                count: *count,
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                mine,
            }
        })
        .collect()
}

/// Reads the reacted-to paragraphs off disk. One file read per scene, not per
/// reaction: a heat map is mostly many marks on few paragraphs.
async fn attach_quotes(state: &AppState, publication: &Publication, blocks: &mut [BlockReactions]) {
    let mut scenes: HashMap<String, serde_json::Value> = HashMap::new();
    for block in blocks.iter_mut() {
        let doc = match scenes.get(&block.scene_id) {
            Some(doc) => doc,
            None => {
                let Ok(doc) = state
                    .store
                    .read_version_scene(
                        &publication.work_id,
                        &publication.version_id,
                        &block.scene_id,
                    )
                    .await
                else {
                    continue;
                };
                scenes.entry(block.scene_id.clone()).or_insert(doc)
            }
        };
        if let Some(text) = crate::domain::doc::block_text(doc, block.block_index as usize) {
            block.quote = text.chars().take(160).collect();
        }
    }
}

// --- the author's questions ----------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateQuestionRequest {
    pub prompt: String,
    #[schema(value_type = crate::domain::enums::QuestionKind)]
    pub kind: String,
    /// `work` asks it once; `chapter` asks it of every chapter on offer.
    #[serde(default = "default_scope")]
    #[schema(value_type = crate::domain::enums::QuestionScope)]
    pub scope: String,
}

fn default_scope() -> String {
    "work".into()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateQuestionRequest {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub position: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/questions",
    tag = "feedback",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "What the author is asking", body = Vec<ReviewQuestion>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_questions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<ReviewQuestion>>> {
    let rows = blocking(&state.pool, move |conn| {
        let (_, work) = support::publication_for_reader(conn, &publication_id, Some(&user))?;
        let mut rows = questions_of(conn, &publication_id)?;
        // Held or removed by moderation: the author still sees theirs, and
        // no reader does.
        if !support::is_author(conn, &work, &user)? {
            rows.retain(|q| q.moderation_state == "visible");
        }
        Ok(rows)
    })
    .await?;
    Ok(Json(rows))
}

fn questions_of(
    conn: &mut crate::db::Conn,
    publication_id: &str,
) -> ApiResult<Vec<ReviewQuestion>> {
    Ok(review_questions::table
        .filter(review_questions::publication_id.eq(publication_id))
        .order((
            review_questions::position.asc(),
            review_questions::created_at.asc(),
        ))
        .select(ReviewQuestion::as_select())
        .load(conn)?)
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/questions",
    tag = "feedback",
    params(("publication_id" = String, Path)),
    request_body = CreateQuestionRequest,
    responses(
        (status = 200, description = "Created", body = ReviewQuestion),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_question(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<CreateQuestionRequest>,
) -> ApiResult<Json<ReviewQuestion>> {
    if body.prompt.trim().is_empty() {
        return Err(ApiError::BadRequest("a question needs a prompt".into()));
    }
    if !QuestionKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            QuestionKind::options()
        )));
    }
    if !QuestionScope::accepts(&body.scope) {
        return Err(ApiError::BadRequest(format!(
            "scope must be one of {}",
            QuestionScope::options()
        )));
    }

    let screen =
        moderation::screen(&state, Surface::Question, &user.id, body.prompt.trim()).await?;
    let row = blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        let next: i64 = review_questions::table
            .filter(review_questions::publication_id.eq(&publication_id))
            .count()
            .get_result(conn)?;
        let now = now_iso();
        let row = NewReviewQuestion {
            id: new_id(),
            publication_id: publication_id.clone(),
            prompt: screen.text.clone(),
            kind: body.kind.clone(),
            scope: body.scope.clone(),
            position: next as i32,
            created_at: now.clone(),
            updated_at: now,
        };
        diesel::insert_into(review_questions::table)
            .values(&row)
            .execute(conn)?;
        let provider = moderation::provider_name(conn)?;
        moderation::record(conn, &screen, "question", &row.id, &user.id, &provider)?;
        Ok(review_questions::table
            .find(&row.id)
            .select(ReviewQuestion::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(row))
}

#[utoipa::path(
    patch,
    path = "/api/questions/{question_id}",
    tag = "feedback",
    params(("question_id" = String, Path)),
    request_body = UpdateQuestionRequest,
    responses(
        (status = 200, description = "Updated", body = ReviewQuestion),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_question(
    State(state): State<AppState>,
    user: AuthUser,
    Path(question_id): Path<String>,
    Json(body): Json<UpdateQuestionRequest>,
) -> ApiResult<Json<ReviewQuestion>> {
    let screen = match body
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        Some(prompt) => {
            Some(moderation::screen(&state, Surface::Question, &user.id, prompt).await?)
        }
        None => None,
    };
    let row = blocking(&state.pool, move |conn| {
        let question = question_for_owner(conn, &question_id, &user)?;
        if let Some(prompt) = body.prompt.as_ref().map(|p| p.trim()) {
            if prompt.is_empty() {
                return Err(ApiError::BadRequest("a question needs a prompt".into()));
            }
            let screen = screen.clone().unwrap_or_default();
            diesel::update(review_questions::table.find(&question.id))
                .set(review_questions::prompt.eq(&screen.text))
                .execute(conn)?;
            let provider = moderation::provider_name(conn)?;
            moderation::record(conn, &screen, "question", &question.id, &user.id, &provider)?;
        }
        if let Some(position) = body.position {
            diesel::update(review_questions::table.find(&question.id))
                .set(review_questions::position.eq(position))
                .execute(conn)?;
        }
        diesel::update(review_questions::table.find(&question.id))
            .set(review_questions::updated_at.eq(now_iso()))
            .execute(conn)?;
        Ok(review_questions::table
            .find(&question.id)
            .select(ReviewQuestion::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(row))
}

#[utoipa::path(
    delete,
    path = "/api/questions/{question_id}",
    tag = "feedback",
    params(("question_id" = String, Path)),
    responses(
        (status = 200, description = "Gone", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_question(
    State(state): State<AppState>,
    user: AuthUser,
    Path(question_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    blocking(&state.pool, move |conn| {
        let question = question_for_owner(conn, &question_id, &user)?;
        diesel::delete(review_questions::table.find(&question.id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

fn question_for_owner(
    conn: &mut crate::db::Conn,
    question_id: &str,
    user: &AuthUser,
) -> ApiResult<ReviewQuestion> {
    let question: ReviewQuestion = review_questions::table
        .find(question_id)
        .select(ReviewQuestion::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such question".into()))?;
    support::publication_for_owner(conn, &question.publication_id, user)?;
    Ok(question)
}

// --- the readers' answers ------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnswerRequest {
    /// Required for a chapter-scoped question, ignored otherwise.
    #[serde(default)]
    pub chapter_id: Option<String>,
    /// 1 to 5, for a rating question.
    #[serde(default)]
    pub rating: Option<i32>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnswerView {
    #[serde(flatten)]
    pub answer: ReviewAnswer,
    pub reader: AuthorCard,
}

#[utoipa::path(
    put,
    path = "/api/questions/{question_id}/answers",
    tag = "feedback",
    params(("question_id" = String, Path)),
    request_body = AnswerRequest,
    responses(
        (status = 200, description = "Recorded", body = AnswerView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn answer(
    State(state): State<AppState>,
    user: AuthUser,
    Path(question_id): Path<String>,
    Json(body): Json<AnswerRequest>,
) -> ApiResult<Json<AnswerView>> {
    let screen = match body
        .body
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Some(text) => Some(moderation::screen(&state, Surface::Answer, &user.id, text).await?),
        None => None,
    };
    let view = blocking(&state.pool, move |conn| {
        let question: ReviewQuestion = review_questions::table
            .find(&question_id)
            .select(ReviewQuestion::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("no such question".into()))?;
        let (publication, work) =
            support::publication_for_reader(conn, &question.publication_id, Some(&user))?;
        if !support::is_author(conn, &work, &user)? {
            support::within_reviewer_cap(conn, &publication, &user)?;
            support::mark_invite_accepted(conn, &publication.id, &user)?;
        }

        let chapter_id = match question.scope.as_str() {
            "chapter" => {
                let chapter_id = body.chapter_id.clone().ok_or_else(|| {
                    ApiError::BadRequest("this question is asked of a chapter".into())
                })?;
                // A chapter the publication does not contain is not a chapter
                // this reader was asked about.
                use crate::db::schema::version_entries;
                let known: i64 = version_entries::table
                    .filter(version_entries::version_id.eq(&publication.version_id))
                    .filter(version_entries::chapter_id.eq(&chapter_id))
                    .count()
                    .get_result(conn)?;
                if known == 0 {
                    return Err(ApiError::BadRequest(
                        "that chapter is not in this publication".into(),
                    ));
                }
                chapter_id
            }
            _ => String::new(),
        };

        match question.kind.as_str() {
            "rating" => match body.rating {
                Some(rating) if (1..=5).contains(&rating) => {}
                _ => return Err(ApiError::BadRequest("rating must be 1 to 5".into())),
            },
            _ => {
                if body.body.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(ApiError::BadRequest("this question wants words".into()));
                }
            }
        }

        let now = now_iso();
        let existing: Option<ReviewAnswer> = review_answers::table
            .filter(review_answers::question_id.eq(&question.id))
            .filter(review_answers::user_id.eq(&user.id))
            .filter(review_answers::chapter_id.eq(&chapter_id))
            .select(ReviewAnswer::as_select())
            .first(conn)
            .optional()?;

        let id = match existing {
            Some(row) => {
                diesel::update(review_answers::table.find(&row.id))
                    .set((
                        review_answers::rating.eq(body.rating),
                        review_answers::body.eq(screen
                            .as_ref()
                            .map(|s| s.text.clone())
                            .or(body.body.clone())),
                        review_answers::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
                row.id
            }
            None => {
                let row = NewReviewAnswer {
                    id: new_id(),
                    question_id: question.id.clone(),
                    user_id: user.id.clone(),
                    chapter_id,
                    rating: body.rating,
                    body: screen
                        .as_ref()
                        .map(|s| s.text.clone())
                        .or(body.body.clone()),
                    created_at: now.clone(),
                    updated_at: now,
                };
                diesel::insert_into(review_answers::table)
                    .values(&row)
                    .execute(conn)?;
                row.id
            }
        };
        if let Some(screen) = &screen {
            let provider = moderation::provider_name(conn)?;
            moderation::record(conn, screen, "answer", &id, &user.id, &provider)?;
        }

        let answer: ReviewAnswer = review_answers::table
            .find(&id)
            .select(ReviewAnswer::as_select())
            .first(conn)?;
        let cards = support::author_cards(conn, std::slice::from_ref(&user.id))?;
        Ok(AnswerView {
            reader: cards.get(&user.id).cloned().unwrap_or(AuthorCard {
                id: user.id.clone(),
                display_name: "Unknown".into(),
            }),
            answer,
        })
    })
    .await?;
    Ok(Json(view))
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/answers",
    tag = "feedback",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Answers so far", body = Vec<AnswerView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
/// The author gets everyone's answers; a reader gets their own back, which is
/// what lets the form reopen with what they already said.
pub async fn list_answers(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<AnswerView>>> {
    let views = blocking(&state.pool, move |conn| {
        let (_, work) = support::publication_for_reader(conn, &publication_id, Some(&user))?;
        let ids: Vec<String> = questions_of(conn, &publication_id)?
            .into_iter()
            .map(|q| q.id)
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut q = review_answers::table
            .filter(review_answers::question_id.eq_any(ids))
            .into_boxed();
        if !support::is_author(conn, &work, &user)? {
            q = q.filter(review_answers::user_id.eq(&user.id));
        } else {
            // The author reads everyone's answers, except ones moderation
            // holds or removed, and ones from an account that is hidden. A
            // reader's own answers always come back to them.
            let hidden = crate::domain::restrictions::hidden_users(conn)?;
            q = q
                .filter(
                    review_answers::moderation_state
                        .eq("visible")
                        .or(review_answers::user_id.eq(&user.id)),
                )
                .filter(review_answers::user_id.ne_all(hidden));
        }
        let rows: Vec<ReviewAnswer> = q
            .order(review_answers::created_at.asc())
            .select(ReviewAnswer::as_select())
            .load(conn)?;

        let reader_ids: Vec<String> = rows.iter().map(|r| r.user_id.clone()).collect();
        let cards = support::author_cards(conn, &reader_ids)?;
        Ok(rows
            .into_iter()
            .map(|answer| AnswerView {
                reader: cards.get(&answer.user_id).cloned().unwrap_or(AuthorCard {
                    id: answer.user_id.clone(),
                    display_name: "Unknown".into(),
                }),
                answer,
            })
            .collect())
    })
    .await?;
    Ok(Json(views))
}

// --- reviewer standing ---------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/users/{user_id}/standing",
    tag = "feedback",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "How useful this reviewer has been", body = support::Standing),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn standing(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<support::Standing>> {
    let standing = blocking(&state.pool, move |conn| {
        let mut all = support::standings(conn, std::slice::from_ref(&user_id))?;
        Ok(all.remove(&user_id).unwrap_or_default())
    })
    .await?;
    Ok(Json(standing))
}
