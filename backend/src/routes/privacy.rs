//! A person's data: the terms it is held under, a copy of all of it, and
//! deleting it.
//!
//! Deletion is the delicate part. Most of what someone made goes with them —
//! their account, the books they started and every file behind those books.
//! But some of it lives inside other people's books: a snapshot a co-author
//! took, the revisions they saved, a note a reader left with replies under it.
//! Cascading those away would damage work that is not theirs, so before the
//! account goes they are handed to the book's owner, or to a placeholder
//! "former member", and anything that is personal — the words of a note — is
//! blanked.
use std::io::{Cursor, Write};

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{self, AuthUser};
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::now_iso;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::support;

/// Raise this with any change of substance to `backend/terms.md`; everyone is
/// asked to agree again.
pub const TERMS_VERSION: i32 = 4;
// Compiled in, so the server never reads it from disk. It lives in backend/
// because the crate is built on its own (the Docker image copies backend/
// alone), and a file outside it is not there to read.
const TERMS_TEXT: &str = include_str!("../../terms.md");

/// Who a deleted account's notes and history are credited to afterwards.
pub const FORMER_MEMBER: &str = "former-member";

/// The placeholder account, created on boot like the built-in templates. Its
/// password hash is not a hash, so nobody can sign in as it.
pub fn seed_former_member(conn: &mut Conn, now: &str) -> ApiResult<()> {
    diesel::insert_or_ignore_into(users::table)
        .values(&NewUser {
            id: FORMER_MEMBER.into(),
            email: "former-member@justtype.invalid".into(),
            display_name: "A former member".into(),
            password_hash: "!".into(),
            bio: None,
            created_at: now.into(),
            updated_at: now.into(),
            daily_word_goal: None,
            legal_name: None,
            mailing_address: None,
            phone: None,
            email_verified_at: None,
            totp_secret: None,
            totp_enabled_at: None,
            totp_last_step: None,
            terms_version: None,
            terms_accepted_at: None,
            weekly_word_goal: None,
            summaries_allowed_at: None,
            assistant_allowed_at: None,
        })
        .execute(conn)?;
    Ok(())
}

// --- terms ---------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Terms {
    pub version: i32,
    /// Markdown.
    pub text: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AcceptTermsRequest {
    /// The version being agreed to — the one the person was shown.
    pub version: i32,
}

#[utoipa::path(
    get,
    path = "/api/terms",
    tag = "trust",
    responses((status = 200, description = "The terms in force", body = Terms))
)]
pub async fn terms() -> Json<Terms> {
    // The comment at the top of the file is a note for whoever edits it.
    let text = match TERMS_TEXT.split_once("-->") {
        Some((_, rest)) if TERMS_TEXT.trim_start().starts_with("<!--") => rest.trim_start(),
        _ => TERMS_TEXT,
    };
    Json(Terms {
        version: TERMS_VERSION,
        text: text.to_string(),
    })
}

#[utoipa::path(
    post,
    path = "/api/me/terms",
    tag = "trust",
    request_body = AcceptTermsRequest,
    responses(
        (status = 200, description = "Recorded", body = User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn accept_terms(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<AcceptTermsRequest>,
) -> ApiResult<Json<User>> {
    if body.version != TERMS_VERSION {
        return Err(ApiError::Conflict(format!(
            "the terms have changed since that page was loaded — version {TERMS_VERSION} is current"
        )));
    }
    let updated = blocking(&state.pool, move |conn| {
        record_terms(conn, &user.id)?;
        support::find_user(conn, &user.id)
    })
    .await?;
    Ok(Json(updated))
}

pub fn record_terms(conn: &mut Conn, user_id: &str) -> ApiResult<()> {
    diesel::update(users::table.find(user_id))
        .set((
            users::terms_version.eq(TERMS_VERSION),
            users::terms_accepted_at.eq(now_iso()),
        ))
        .execute(conn)?;
    super::audit::record(
        conn,
        Some(user_id),
        None,
        AuditAction::TermsAccepted,
        &format!("version {TERMS_VERSION}"),
    )
}

// --- a copy of everything ------------------------------------------------

/// Everything held about the caller, as one zip they can open without us:
/// JSON for the records, and the manuscript files exactly as they are stored.
#[utoipa::path(
    get,
    path = "/api/me/export",
    tag = "trust",
    responses(
        (status = 200, description = "A zip of everything", content_type = "application/zip"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export(State(state): State<AppState>, user: AuthUser) -> ApiResult<impl IntoResponse> {
    let bytes = export_zip(&state, &user.id).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"justtype-export.zip\""),
    );
    Ok((StatusCode::OK, headers, bytes))
}

/// The zip itself: the code behind `/api/me/export`, and behind the copy
/// emailed to someone who can't sign in to ask for it.
pub async fn export_zip(state: &AppState, user_id: &str) -> ApiResult<Vec<u8>> {
    let uid = user_id.to_string();
    let (records, owned) = blocking(&state.pool, move |conn| {
        let records = gather(conn, &uid)?;
        let owned: Vec<(String, String)> = works::table
            .filter(works::owner_id.eq(&uid))
            .select((works::id, works::title))
            .load(conn)?;
        super::audit::record(
            conn,
            Some(&uid),
            None,
            AuditAction::AccountExported,
            &format!("{} work(s)", owned.len()),
        )?;
        Ok((records, owned))
    })
    .await?;

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let zipped =
        |e: zip::result::ZipError| ApiError::internal(format!("could not build the zip: {e}"));

    zip.start_file("README.txt", options).map_err(zipped)?;
    zip.write_all(README.as_bytes())?;
    for (name, value) in records {
        zip.start_file(format!("{name}.json"), options)
            .map_err(zipped)?;
        zip.write_all(serde_json::to_string_pretty(&value)?.as_bytes())?;
    }
    for (work_id, title) in &owned {
        let folder = format!(
            "manuscripts/{}-{}",
            slug(title),
            &work_id[..8.min(work_id.len())]
        );
        for (path, bytes) in state.store.work_files(work_id).await? {
            zip.start_file(format!("{folder}/{path}"), options)
                .map_err(zipped)?;
            zip.write_all(&bytes)?;
        }
    }
    Ok(zip.finish().map_err(zipped)?.into_inner())
}

/// Builds `user_id`'s export, keeps it for `days`, and queues an email with a
/// link to it that works without signing in. For someone who can't sign in
/// (banned) or whose account is about to go (deleted by staff).
pub async fn email_export(
    state: &AppState,
    user_id: &str,
    to: &str,
    name: &str,
    days: i64,
) -> ApiResult<()> {
    let bytes = export_zip(state, user_id).await?;
    let id = crate::domain::new_id();
    let dir = state.config.data_dir.join("emailed-exports");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{id}.zip"));
    std::fs::write(&path, bytes)?;
    let token = auth::generate_link_token();
    let (to, name) = (to.to_string(), name.to_string());
    let public_url = state.config.public_url.clone();
    let stored = path.to_string_lossy().to_string();
    blocking(&state.pool, move |conn| {
        diesel::insert_into(emailed_exports::table)
            .values((
                emailed_exports::id.eq(&id),
                emailed_exports::token_hash.eq(auth::hash_refresh_token(&token)),
                emailed_exports::email.eq(&to),
                emailed_exports::path.eq(&stored),
                emailed_exports::expires_at.eq((crate::clock::now()
                    + chrono::Duration::days(days))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
                emailed_exports::created_at.eq(crate::clock::now_iso()),
            ))
            .execute(conn)?;
        crate::mail::enqueue(
            conn,
            &crate::mail::Message {
                to: to.clone(),
                subject: "A copy of your Just Type Studio work".into(),
                body: format!(
                    "Hello {name},\n\nHere is everything we hold for your account, as one zip you can open without us:\n\n{public_url}/api/ban/export/{token}\n\nThe link works for {days} days.\n"
                ),
                html: None,
                headers: Vec::new(),
            },
            crate::clock::now() + chrono::Duration::days(3),
        )
    })
    .await
}

const README: &str = "Everything Just Type Studio holds about you, as of the moment this was made.

account.json        your account, sessions, the people you follow and have blocked,
                    your reader circles, your writing days and the words you
                    added to the spellchecker
works.json          every work you started: its outline, snapshots, publications,
                    the feedback left on them, its notebook, to-do list, co-authors
                    and activity
contributions.json  what you did on other people's work: notes, votes, reactions,
                    answers, how far you read, changes you proposed, books you
                    were added to, and your own highlights and bookmarks
exports.json        your saved export profiles, the settings last used for each
                    book, and the record of what you exported and when
prompts.json        Daily Prompts: whether you take part and your email
                    preferences, every answer you wrote (each a TipTap
                    document), the critiques and reports you wrote, the days
                    the prompt was emailed to you, and your inbox
audit.json          the record of what you did to your account and your works
manuscripts/        the files behind each work you started, exactly as stored:
                    working/ is the current draft of each scene, revisions/ its
                    saved history, versions/ every snapshot. Each is a TipTap
                    (ProseMirror) JSON document.

Your password is not here: only a one-way hash of it was ever kept, and that
is not yours to need.
";

fn slug(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "untitled".into()
    } else {
        slug.chars().take(60).collect()
    }
}

fn gather(conn: &mut Conn, user_id: &str) -> ApiResult<Vec<(&'static str, Value)>> {
    let user = support::find_user(conn, user_id)?;

    // --- account
    let my_sessions: Vec<Session> = sessions::table
        .filter(sessions::user_id.eq(user_id))
        .select(Session::as_select())
        .load(conn)?;
    let following: Vec<String> = follows::table
        .filter(follows::follower_id.eq(user_id))
        .select(follows::followee_id)
        .load(conn)?;
    let blocked: Vec<BlockedReader> = blocked_readers::table
        .filter(blocked_readers::owner_id.eq(user_id))
        .select(BlockedReader::as_select())
        .load(conn)?;
    let circles: Vec<(String, String)> = reader_circles::table
        .filter(reader_circles::owner_id.eq(user_id))
        .select((reader_circles::id, reader_circles::name))
        .load(conn)?;
    let circle_ids: Vec<String> = circles.iter().map(|c| c.0.clone()).collect();
    let members: Vec<(String, String, Option<String>)> = circle_members::table
        .filter(circle_members::circle_id.eq_any(&circle_ids))
        .select((
            circle_members::circle_id,
            circle_members::email,
            circle_members::name,
        ))
        .load(conn)?;
    let days: Vec<(String, i32, i32)> = writing_days::table
        .filter(writing_days::user_id.eq(user_id))
        .order(writing_days::day.asc())
        .select((
            writing_days::day,
            writing_days::net_words,
            writing_days::saves,
        ))
        .load(conn)?;
    let dictionary: Vec<String> = dictionary_words::table
        .filter(dictionary_words::user_id.eq(user_id))
        .order(dictionary_words::word.asc())
        .select(dictionary_words::word)
        .load(conn)?;
    let account = json!({
        "user": user,
        "sessions": my_sessions,
        "following": support::author_cards(conn, &following)?.into_values().collect::<Vec<_>>(),
        "blocked": blocked,
        "circles": circles.iter().map(|(id, name)| json!({
            "name": name,
            "members": members.iter().filter(|m| &m.0 == id)
                .map(|m| json!({ "email": m.1, "name": m.2 })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "writingDays": days.iter().map(|(day, words, saves)| json!({
            "day": day, "netWords": words, "saves": saves,
        })).collect::<Vec<_>>(),
        "dictionary": dictionary,
    });

    // --- works they started
    let owned: Vec<Work> = works::table
        .filter(works::owner_id.eq(user_id))
        .select(Work::as_select())
        .load(conn)?;
    let mut works_out = Vec::new();
    for work in owned {
        let outline = super::works::load_outline(conn, &work.id)?;
        let work_versions: Vec<Version> = versions::table
            .filter(versions::work_id.eq(&work.id))
            .order(versions::number.asc())
            .select(Version::as_select())
            .load(conn)?;
        let pubs: Vec<Publication> = publications::table
            .filter(publications::work_id.eq(&work.id))
            .select(Publication::as_select())
            .load(conn)?;
        let pub_ids: Vec<String> = pubs.iter().map(|p| p.id.clone()).collect();
        let feedback: Vec<Comment> = comments::table
            .filter(comments::publication_id.eq_any(&pub_ids))
            .filter(comments::state.ne("deleted"))
            .order(comments::created_at.asc())
            .select(Comment::as_select())
            .load(conn)?;
        let names: Vec<String> = feedback.iter().map(|c| c.author_id.clone()).collect();
        let names = support::author_cards(conn, &names)?;
        let notebook: Vec<NotebookEntry> = notebook_entries::table
            .filter(notebook_entries::work_id.eq(&work.id))
            .select(NotebookEntry::as_select())
            .load(conn)?;
        let tasks: Vec<RevisionTask> = revision_tasks::table
            .filter(revision_tasks::work_id.eq(&work.id))
            .select(RevisionTask::as_select())
            .load(conn)?;
        let collaborators: Vec<WorkCollaborator> = work_collaborators::table
            .filter(work_collaborators::work_id.eq(&work.id))
            .select(WorkCollaborator::as_select())
            .load(conn)?;
        let activity: Vec<WorkActivity> = work_activity::table
            .filter(work_activity::work_id.eq(&work.id))
            .order(work_activity::created_at.asc())
            .select(WorkActivity::as_select())
            .load(conn)?;
        works_out.push(json!({
            "outline": outline,
            "versions": work_versions,
            "publications": pubs,
            // Other people's notes on your book, by name only: their
            // addresses are theirs.
            "feedback": feedback.iter().map(|c| json!({
                "comment": c,
                "author": names.get(&c.author_id).map(|a| a.display_name.clone()),
            })).collect::<Vec<_>>(),
            "notebook": notebook,
            "tasks": tasks,
            "collaborators": collaborators,
            "activity": activity,
        }));
    }

    // --- what they did elsewhere
    let notes: Vec<Comment> = comments::table
        .filter(comments::author_id.eq(user_id))
        .select(Comment::as_select())
        .load(conn)?;
    let votes: Vec<(String, i32, String)> = comment_votes::table
        .filter(comment_votes::user_id.eq(user_id))
        .select((
            comment_votes::comment_id,
            comment_votes::value,
            comment_votes::created_at,
        ))
        .load(conn)?;
    let my_reactions: Vec<(String, String, i32, String, String)> = reactions::table
        .filter(reactions::user_id.eq(user_id))
        .select((
            reactions::publication_id,
            reactions::scene_id,
            reactions::block_index,
            reactions::kind,
            reactions::created_at,
        ))
        .load(conn)?;
    let answers: Vec<ReviewAnswer> = review_answers::table
        .filter(review_answers::user_id.eq(user_id))
        .select(ReviewAnswer::as_select())
        .load(conn)?;
    let progress: Vec<ReadingProgress> = reading_progress::table
        .filter(reading_progress::user_id.eq(user_id))
        .select(ReadingProgress::as_select())
        .load(conn)?;
    let proposals: Vec<TrackedChange> = tracked_changes::table
        .filter(tracked_changes::author_id.eq(user_id))
        .select(TrackedChange::as_select())
        .load(conn)?;
    let marks: Vec<Annotation> = annotations::table
        .filter(annotations::user_id.eq(user_id))
        .order(annotations::created_at.asc())
        .select(Annotation::as_select())
        .load(conn)?;
    let seats: Vec<(String, String, String)> = work_collaborators::table
        .inner_join(works::table)
        .filter(work_collaborators::user_id.eq(user_id))
        .select((works::id, works::title, work_collaborators::role))
        .load(conn)?;
    let contributions = json!({
        "notes": notes,
        "votes": votes.iter().map(|(c, v, at)| json!({ "commentId": c, "value": v, "at": at }))
            .collect::<Vec<_>>(),
        "reactions": my_reactions.iter().map(|(p, s, b, k, at)| json!({
            "publicationId": p, "sceneId": s, "blockIndex": b, "kind": k, "at": at,
        })).collect::<Vec<_>>(),
        "answers": answers,
        "readingProgress": progress,
        "proposedChanges": proposals,
        "highlightsAndBookmarks": marks,
        "coAuthoring": seats.iter().map(|(id, title, role)| json!({
            "workId": id, "title": title, "role": role,
        })).collect::<Vec<_>>(),
    });

    let audit: Vec<AuditEvent> = audit_events::table
        .filter(audit_events::actor_id.eq(user_id))
        .order(audit_events::created_at.asc())
        .select(AuditEvent::as_select())
        .load(conn)?;

    // How they like their exports, and what they have sent out.
    let profiles: Vec<ExportProfile> = export_profiles::table
        .filter(export_profiles::user_id.eq(user_id))
        .order(export_profiles::created_at.asc())
        .select(ExportProfile::as_select())
        .load(conn)?;
    let history: Vec<ExportHistoryRow> = export_history::table
        .filter(export_history::user_id.eq(user_id))
        .order(export_history::created_at.asc())
        .select(ExportHistoryRow::as_select())
        .load(conn)?;
    let last: Vec<(String, String, String, String)> = export_last_options::table
        .inner_join(works::table)
        .filter(works::owner_id.eq(user_id))
        .select((
            export_last_options::work_id,
            export_last_options::format,
            export_last_options::options,
            export_last_options::used_at,
        ))
        .load(conn)?;
    let exports = json!({
        "profiles": profiles,
        "history": history,
        "lastUsed": last.iter().map(|(work, format, options, at)| json!({
            "workId": work,
            "format": format,
            "options": serde_json::from_str::<Value>(options).unwrap_or(Value::Null),
            "usedAt": at,
        })).collect::<Vec<_>>(),
    });

    // Daily Prompts.
    let enrollment: Option<PromptEnrollment> = prompt_enrollments::table
        .find(user_id)
        .select(PromptEnrollment::as_select())
        .first(conn)
        .optional()?;
    let entries: Vec<(PromptEntry, Option<String>, String)> = prompt_entries::table
        .inner_join(daily_prompts::table)
        .filter(prompt_entries::author_id.eq(user_id))
        .order(prompt_entries::created_at.asc())
        .select((
            PromptEntry::as_select(),
            daily_prompts::day,
            daily_prompts::body,
        ))
        .load(conn)?;
    let critiques: Vec<PromptCritique> = prompt_critiques::table
        .filter(prompt_critiques::author_id.eq(user_id))
        .order(prompt_critiques::created_at.asc())
        .select(PromptCritique::as_select())
        .load(conn)?;
    let reports: Vec<PromptCritiqueReport> = prompt_critique_reports::table
        .filter(prompt_critique_reports::reporter_id.eq(user_id))
        .select(PromptCritiqueReport::as_select())
        .load(conn)?;
    let emailed: Vec<(String, String)> = prompt_email_sends::table
        .filter(prompt_email_sends::user_id.eq(user_id))
        .order(prompt_email_sends::day.asc())
        .select((prompt_email_sends::day, prompt_email_sends::sent_at))
        .load(conn)?;
    let prompts_out = json!({
        "enrollment": enrollment,
        "entries": entries.iter().map(|(e, day, prompt)| json!({
            "entry": e,
            "day": day,
            "prompt": prompt,
            "content": serde_json::from_str::<Value>(&e.body).unwrap_or(Value::Null),
        })).collect::<Vec<_>>(),
        "critiques": critiques,
        "reportsFiled": reports,
        "emailedOn": emailed.iter().map(|(day, at)| json!({ "day": day, "sentAt": at })).collect::<Vec<_>>(),
        "inbox": super::inbox::export_of(conn, user_id)?,
    });

    Ok(vec![
        ("account", account),
        ("prompts", prompts_out),
        ("works", Value::Array(works_out)),
        ("contributions", contributions),
        ("exports", exports),
        ("audit", serde_json::to_value(audit)?),
    ])
}

// --- deleting it ---------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAccountRequest {
    pub password: String,
    /// From the authenticator, when two-factor sign-in is on.
    #[serde(default)]
    pub code: Option<String>,
    /// Required when a work being deleted has co-authors on it, after they
    /// have been told which.
    #[serde(default)]
    pub delete_shared_works: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeletedAccount {
    pub works_deleted: usize,
}

#[utoipa::path(
    delete,
    path = "/api/me",
    tag = "trust",
    request_body = DeleteAccountRequest,
    responses(
        (status = 200, description = "Gone", body = DeletedAccount),
        (status = 409, description = "Works with co-authors would go too", body = ErrorEnvelope),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_account(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DeleteAccountRequest>,
) -> ApiResult<Json<DeletedAccount>> {
    state.limiter.check(
        &user.id,
        Action::SecondFactor,
        state.config.second_factor_rate,
    )?;
    let owned = blocking(&state.pool, move |conn| {
        let found = support::find_user(conn, &user.id)?;
        if !auth::password::verify(&body.password, &found.password_hash) {
            return Err(ApiError::Unauthorized("that is not your password".into()));
        }
        if found.totp_enabled_at.is_some() {
            let code = body.code.as_deref().unwrap_or_default();
            if code.trim().is_empty() {
                return Err(ApiError::TwoFactorRequired(
                    "enter the code from your authenticator app".into(),
                ));
            }
            super::account::check_second_factor(conn, &found, code)?;
        }

        let owned: Vec<(String, String)> = works::table
            .filter(works::owner_id.eq(&user.id))
            .select((works::id, works::title))
            .load(conn)?;
        let owned_ids: Vec<String> = owned.iter().map(|w| w.0.clone()).collect();
        let shared: Vec<String> = work_collaborators::table
            .inner_join(works::table)
            .filter(works::id.eq_any(&owned_ids))
            .select(works::title)
            .distinct()
            .load(conn)?;
        if !shared.is_empty() && !body.delete_shared_works {
            return Err(ApiError::ConflictWith {
                message: format!(
                    "other people are working on {} — deleting your account deletes {} for them too",
                    shared.join(", "),
                    if shared.len() == 1 { "it" } else { "them" }
                ),
                details: json!({ "sharedWorks": shared }),
            });
        }

        conn.transaction::<_, ApiError, _>(|conn| {
            let removed = remove_account(conn, &found, false)?;
            super::audit::record(
                conn,
                Some(&found.id),
                None,
                AuditAction::AccountDeleted,
                &format!("{} work(s) and their files deleted", removed.deleted.len()),
            )?;
            for (work_id, title) in &removed.deleted {
                super::audit::record(
                    conn,
                    Some(&found.id),
                    Some(work_id),
                    AuditAction::WorkDeleted,
                    &format!("{title}, with the account that started it"),
                )?;
            }
            Ok(())
        })?;
        Ok(owned_ids)
    })
    .await?;

    // The rows are gone; now the files. A failure here leaves orphaned files
    // rather than orphaned rows, and is logged loudly so it can be swept.
    let count = owned.len();
    for work_id in owned {
        if let Err(e) = state.store.delete_work(&work_id).await {
            tracing::error!(%work_id, error = %e, "account deleted but its work's files remain");
        }
    }
    Ok(Json(DeletedAccount {
        works_deleted: count,
    }))
}

/// What deleting an account removed and passed on.
pub struct Removed {
    /// Works deleted with the account: `(id, title)`.
    pub deleted: Vec<(String, String)>,
    /// Co-written works that passed to a collaborator: `(id, title, new owner)`.
    pub handed_over: Vec<(String, String, String)>,
}

/// Titles of the works this account owns that other people are working on.
pub fn shared_titles(conn: &mut Conn, user_id: &str) -> ApiResult<Vec<String>> {
    Ok(work_collaborators::table
        .inner_join(works::table)
        .filter(works::owner_id.eq(user_id))
        .select(works::title)
        .distinct()
        .load(conn)?)
}

/// Deletes the account and everything that belongs to it, inside the caller's
/// transaction. The one code path behind self-service deletion, deletion by
/// staff, and the purge that follows a closing restriction.
///
/// With `hand_shared`, a book the account owns that has other collaborators
/// passes to the one who joined it first instead of being deleted; the
/// account's own contributions inside it stay, credited as `hand_over` credits
/// them. Without it, those books go with the account (the caller has already
/// asked whether that is what was meant).
pub fn remove_account(conn: &mut Conn, found: &User, hand_shared: bool) -> ApiResult<Removed> {
    seed_former_member(conn, &now_iso())?;
    let owned: Vec<(String, String)> = works::table
        .filter(works::owner_id.eq(&found.id))
        .select((works::id, works::title))
        .load(conn)?;
    let mut handed_over = Vec::new();
    let mut deleted = Vec::new();
    for (work_id, title) in owned {
        let heir: Option<(String, String)> = if hand_shared {
            work_collaborators::table
                .filter(work_collaborators::work_id.eq(&work_id))
                .filter(work_collaborators::user_id.is_not_null())
                .filter(work_collaborators::user_id.ne(&found.id))
                .filter(work_collaborators::accepted_at.is_not_null())
                .order((
                    work_collaborators::accepted_at.asc(),
                    work_collaborators::created_at.asc(),
                ))
                .select((
                    work_collaborators::id,
                    work_collaborators::user_id.assume_not_null(),
                ))
                .first(conn)
                .optional()?
        } else {
            None
        };
        match heir {
            Some((row_id, heir_id)) => {
                diesel::update(works::table.find(&work_id))
                    .set(works::owner_id.eq(&heir_id))
                    .execute(conn)?;
                // The owner is the owner; they don't also need a seat.
                diesel::delete(work_collaborators::table.find(&row_id)).execute(conn)?;
                handed_over.push((work_id, title, heir_id));
            }
            None => deleted.push((work_id, title)),
        }
    }
    let deleted_ids: Vec<String> = deleted.iter().map(|w| w.0.clone()).collect();
    hand_over(conn, found, &deleted_ids)?;
    diesel::delete(users::table.find(&found.id)).execute(conn)?;
    Ok(Removed {
        deleted,
        handed_over,
    })
}

/// Moves what the departing account made inside other people's books to
/// someone who will still be here, and blanks what was personal.
fn hand_over(conn: &mut Conn, user: &User, owned: &[String]) -> ApiResult<()> {
    let gone = user.id.as_str();

    // Snapshots, saved revisions and invitations on books they did not start
    // belong to those books: credited to the book's owner rather than cascaded
    // away with the account.
    let elsewhere: Vec<(String, String)> = works::table
        .filter(works::id.ne_all(owned))
        .filter(
            works::id
                .eq_any(
                    versions::table
                        .filter(versions::created_by.eq(gone))
                        .select(versions::work_id),
                )
                .or(works::id.eq_any(
                    scene_revisions::table
                        .filter(scene_revisions::author_id.eq(gone))
                        .select(scene_revisions::work_id),
                ))
                .or(works::id.eq_any(
                    publications::table
                        .inner_join(publication_invites::table)
                        .filter(publication_invites::invited_by.eq(gone))
                        .select(publications::work_id),
                )),
        )
        .select((works::id, works::owner_id))
        .load(conn)?;
    for (work_id, owner_id) in &elsewhere {
        diesel::update(
            versions::table
                .filter(versions::work_id.eq(work_id))
                .filter(versions::created_by.eq(gone)),
        )
        .set(versions::created_by.eq(owner_id))
        .execute(conn)?;
        diesel::update(
            scene_revisions::table
                .filter(scene_revisions::work_id.eq(work_id))
                .filter(scene_revisions::author_id.eq(gone)),
        )
        .set(scene_revisions::author_id.eq(owner_id))
        .execute(conn)?;
        let pubs = publications::table
            .filter(publications::work_id.eq(work_id))
            .select(publications::id);
        diesel::update(
            publication_invites::table
                .filter(publication_invites::publication_id.eq_any(pubs))
                .filter(publication_invites::invited_by.eq(gone)),
        )
        .set(publication_invites::invited_by.eq(owner_id))
        .execute(conn)?;
    }

    // Daily Prompts. Their own answers go with the account (by cascade), and
    // the critiques on them; first the inbox items other people hold about
    // those answers, which nothing would otherwise remove.
    let my_entries: Vec<String> = prompt_entries::table
        .filter(prompt_entries::author_id.eq(gone))
        .select(prompt_entries::id)
        .load(conn)?;
    diesel::delete(
        inbox_items::table
            .filter(inbox_items::context_type.eq("prompt_entry"))
            .filter(inbox_items::context_id.eq_any(&my_entries)),
    )
    .execute(conn)?;
    // Critiques on other people's answers keep their place, as notes do, but
    // lose their words and their author, and nobody is still told about them.
    let my_critiques = prompt_critiques::table
        .filter(prompt_critiques::author_id.eq(gone))
        .select(prompt_critiques::id);
    diesel::delete(
        inbox_items::table
            .filter(inbox_items::subject_type.eq("prompt_critique"))
            .filter(inbox_items::subject_id.eq_any(my_critiques)),
    )
    .execute(conn)?;
    diesel::update(prompt_critiques::table.filter(prompt_critiques::author_id.eq(gone)))
        .set((
            prompt_critiques::author_id.eq(FORMER_MEMBER),
            prompt_critiques::body.eq(""),
            prompt_critiques::deleted_at.eq(Some(now_iso())),
        ))
        .execute(conn)?;

    // Notes keep their place so the replies under them still read, but lose
    // their words and their author.
    diesel::update(comments::table.filter(comments::author_id.eq(gone)))
        .set((
            comments::author_id.eq(FORMER_MEMBER),
            comments::body.eq(""),
            comments::anchor_quote.eq(None::<String>),
            comments::state.eq("deleted"),
        ))
        .execute(conn)?;

    // Votes go; the scores they were part of are recounted, since a score is
    // only ever the sum of the votes still standing.
    let voted: Vec<String> = comment_votes::table
        .filter(comment_votes::user_id.eq(gone))
        .select(comment_votes::comment_id)
        .load(conn)?;
    diesel::delete(comment_votes::table.filter(comment_votes::user_id.eq(gone))).execute(conn)?;
    for comment_id in voted {
        let total: Option<i64> = comment_votes::table
            .filter(comment_votes::comment_id.eq(&comment_id))
            .select(diesel::dsl::sum(comment_votes::value))
            .first(conn)?;
        diesel::update(comments::table.find(&comment_id))
            .set(comments::score.eq(total.unwrap_or(0) as i32))
            .execute(conn)?;
    }

    // What they did on other people's books stays in those books' feeds, by
    // a name that is no longer theirs.
    diesel::update(work_activity::table.filter(work_activity::actor_id.eq(gone)))
        .set(work_activity::actor_id.eq(FORMER_MEMBER))
        .execute(conn)?;

    // Their address, wherever it was used to let them in.
    diesel::delete(
        work_collaborators::table.filter(
            work_collaborators::user_id
                .eq(gone)
                .or(work_collaborators::email.eq(&user.email)),
        ),
    )
    .execute(conn)?;
    diesel::delete(publication_invites::table.filter(publication_invites::email.eq(&user.email)))
        .execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe_and_readable() {
        assert_eq!(slug("The Long Field"), "the-long-field");
        assert_eq!(slug("  ¿Qué?  "), "qu");
        assert_eq!(slug("…"), "untitled");
    }

    #[test]
    fn the_terms_say_the_author_keeps_their_copyright() {
        assert!(TERMS_TEXT.contains("belongs to you"));
        assert!(TERMS_TEXT.contains("does not transfer, license or share your\ncopyright"));
    }
}
