//! Asking someone else to come and write here.
//!
//! The server sends the mail, under our name, to an address it was handed by a
//! writer, so this is the one route that lets a stranger's inbox be reached
//! from the outside. It is fenced accordingly: only a confirmed address can
//! invite, a writer has a daily allowance, one writer invites one address once,
//! and an address that anyone invited lately is not mailed again for a while.
//!
//! None of that is allowed to tell the inviter whether an address already has
//! an account. Sign-in and reset are careful not to, and an invite form that
//! answered "already a member" would undo them. So every address is recorded
//! and counted the same, the reply is the same, and only the mail differs.
use axum::extract::State;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::schema::{site_invites, users};
use crate::db::{blocking, Conn};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::mail::Message;
use crate::state::AppState;

use super::auth::normalize_email;

/// Addresses in one request.
const PER_REQUEST: usize = 10;
/// Addresses one writer may invite in any 24 hours.
const PER_DAY: i64 = 20;
/// Once an address has been mailed an invitation, nobody's mails it again for
/// this long: ten writers who know the same person send one email, not ten.
const RECIPIENT_QUIET_DAYS: i64 = 30;
/// How long the queue keeps trying to deliver one.
const GIVE_UP_DAYS: i64 = 3;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InviteRequest {
    pub emails: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InviteResponse {
    /// The addresses as understood: trimmed, lowercased, each once. The same
    /// whether or not they were mailed, for the reason at the top.
    pub invited: Vec<String>,
    /// How many more addresses the writer can invite today.
    pub remaining_today: i64,
}

/// Lowercased and checked a little harder than sign-up does, because this one
/// goes into a `To:` header on a stranger's behalf.
fn clean(raw: &str) -> ApiResult<String> {
    let email = normalize_email(raw)?;
    let bad = email.len() > 254
        || email.matches('@').count() != 1
        || email.starts_with('@')
        || !email.rsplit('@').next().unwrap_or("").contains('.')
        || email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ",;<>\"()[]\\".contains(c));
    if bad {
        return Err(ApiError::BadRequest(format!(
            "{} is not an email address",
            raw.trim()
        )));
    }
    Ok(email)
}

fn ago(days: i64) -> String {
    (crate::clock::now() - chrono::Duration::days(days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn sent_today(conn: &mut Conn, inviter_id: &str) -> ApiResult<i64> {
    Ok(site_invites::table
        .filter(site_invites::inviter_id.eq(inviter_id))
        .filter(site_invites::created_at.gt(ago(1)))
        .count()
        .get_result(conn)?)
}

#[utoipa::path(
    post,
    path = "/api/invites",
    tag = "account",
    request_body = InviteRequest,
    responses(
        (status = 200, description = "Invited", body = InviteResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
/// Invites people to Just Type Studio by email.
pub async fn invite(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<InviteRequest>,
) -> ApiResult<Json<InviteResponse>> {
    if !user.email_verified {
        return Err(ApiError::Forbidden(
            "confirm your own address before inviting anyone".into(),
        ));
    }
    let mut emails: Vec<String> = Vec::new();
    for raw in body.emails.iter().filter(|e| !e.trim().is_empty()) {
        let email = clean(raw)?;
        if email == user.email {
            return Err(ApiError::BadRequest("that is your own address".into()));
        }
        if !emails.contains(&email) {
            emails.push(email);
        }
    }
    if emails.is_empty() {
        return Err(ApiError::BadRequest("add an address to invite".into()));
    }
    if emails.len() > PER_REQUEST {
        return Err(ApiError::BadRequest(format!(
            "{PER_REQUEST} addresses at a time, please"
        )));
    }

    let public_url = state.config.public_url.clone();
    let remaining_today = blocking(&state.pool, {
        let emails = emails.clone();
        move |conn| {
            conn.transaction::<_, ApiError, _>(|conn| {
                let inviter: String = users::table
                    .find(&user.id)
                    .select(users::display_name)
                    .first(conn)?;
                let already: Vec<String> = site_invites::table
                    .filter(site_invites::inviter_id.eq(&user.id))
                    .filter(site_invites::email.eq_any(&emails))
                    .select(site_invites::email)
                    .load(conn)?;
                // Asking again for someone already asked is not a new
                // invitation, so it neither mails nor counts.
                let fresh: Vec<&String> = emails.iter().filter(|e| !already.contains(e)).collect();
                let used = sent_today(conn, &user.id)?;
                if used + fresh.len() as i64 > PER_DAY {
                    return Err(ApiError::TooManyRequests {
                        message: format!("you can invite {} more today", (PER_DAY - used).max(0)),
                        retry_after: 24 * 60 * 60,
                    });
                }
                for email in fresh {
                    let member = users::table
                        .filter(users::email.eq(email))
                        .count()
                        .get_result::<i64>(conn)?
                        > 0;
                    let lately = site_invites::table
                        .filter(site_invites::email.eq(email))
                        .filter(site_invites::mailed.eq(1))
                        .filter(site_invites::created_at.gt(ago(RECIPIENT_QUIET_DAYS)))
                        .count()
                        .get_result::<i64>(conn)?
                        > 0;
                    let mail = !member && !lately;
                    diesel::insert_into(site_invites::table)
                        .values((
                            site_invites::id.eq(new_id()),
                            site_invites::inviter_id.eq(&user.id),
                            site_invites::email.eq(email),
                            site_invites::mailed.eq(mail as i32),
                            site_invites::created_at.eq(now_iso()),
                        ))
                        .execute(conn)?;
                    if mail {
                        crate::mail::enqueue(
                            conn,
                            &invite_message(&public_url, email, &inviter),
                            chrono::Utc::now() + chrono::Duration::days(GIVE_UP_DAYS),
                        )?;
                    }
                }
                Ok(PER_DAY - sent_today(conn, &user.id)?)
            })
        }
    })
    .await?;
    Ok(Json(InviteResponse {
        invited: emails,
        remaining_today,
    }))
}

/// The invitation. Fixed words — the inviter chooses who, never what — so it
/// cannot be used to put someone else's message in a stranger's inbox under
/// our name.
pub fn invite_message(public_url: &str, to: &str, inviter: &str) -> Message {
    let join = format!("{public_url}/sign-in?invited=1");
    let body = format!(
        "Hello,\n\n\
{inviter} writes on Just Type Studio and thought you might like to as well.\n\n\
It is a quiet place to write something long: a novel, a novella, a story. \
You write scene by scene, it saves as you go, and when a draft is ready you \
can put it in front of readers you choose. There is a prompt a day, too, if \
you want a reason to write every day.\n\n\
Make an account:\n{join}\n\n\
You are getting this because {inviter} typed your address. They can only \
do that once, and if you do nothing we will not write to you about it again.\n"
    );
    let button = "display:inline-block;padding:10px 16px;background:#7a4f2a;color:#fff;text-decoration:none;border-radius:4px";
    let html = format!(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;color:#222;max-width:560px;margin:0 auto;padding:24px;line-height:1.5\">\
<p>Hello,</p>\
<p>{inviter} writes on Just Type Studio and thought you might like to as well.</p>\
<p>It is a quiet place to write something long: a novel, a novella, a story. \
You write scene by scene, it saves as you go, and when a draft is ready you can put it \
in front of readers you choose. There is a prompt a day, too, if you want a reason to write every day.</p>\
<p><a href=\"{join}\" style=\"{button}\">Make an account</a></p>\
<p style=\"color:#666;font-size:13px\">You are getting this because {inviter} typed your address. \
They can only do that once, and if you do nothing we will not write to you about it again.</p>\
</body></html>",
        inviter = crate::jobs::escape(inviter),
        join = crate::jobs::escape(&join),
    );
    Message {
        to: to.into(),
        subject: format!("{inviter} invited you to Just Type Studio"),
        body,
        html: Some(html),
        headers: Vec::new(),
    }
}
