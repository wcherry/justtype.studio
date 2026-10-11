//! The parts of an account that are not writing: proving the address on it,
//! getting back in when the password is gone, a second factor, and the list of
//! places it is signed in.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::{self, AuthUser};
use crate::db::models::*;
use crate::db::schema::{email_tokens, recovery_codes, refresh_tokens, sessions, users};
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::mail::Message;
use crate::state::AppState;

use super::auth::{check_password, normalize_email};
use super::support;

/// How long a link to prove an address stays good. Long enough to find it the
/// next morning.
const VERIFY_HOURS: i64 = 72;
/// How long a reset link stays good. Short: it is a key to the account.
const RESET_MINUTES: i64 = 60;
/// Mail to one address per minute.
const MAIL_PER_MINUTE: i64 = 3;
const RECOVERY_CODES: usize = 10;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenRequest {
    pub token: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForgotRequest {
    pub email: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetRequest {
    pub token: String,
    pub password: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorSetup {
    /// The key to type into an authenticator by hand.
    pub secret: String,
    /// The same key as an `otpauth://` link, for a QR code or a tap.
    pub url: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeRequest {
    pub code: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisableTwoFactorRequest {
    pub password: String,
    /// From the authenticator, or a recovery code.
    pub code: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCodes {
    /// Shown once. Each gets someone in once if the authenticator is lost.
    pub codes: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    #[serde(flatten)]
    pub session: Session,
    /// The session this request came from.
    pub current: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub ok: bool,
}

fn later(duration: chrono::Duration) -> String {
    (chrono::Utc::now() + duration).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn mint(conn: &mut Conn, user: &User, purpose: &str, lasts: chrono::Duration) -> ApiResult<String> {
    // A new link retires the old ones: only the latest mail should work.
    diesel::update(
        email_tokens::table
            .filter(email_tokens::user_id.eq(&user.id))
            .filter(email_tokens::purpose.eq(purpose))
            .filter(email_tokens::used_at.is_null()),
    )
    .set(email_tokens::used_at.eq(now_iso()))
    .execute(conn)?;
    let token = auth::generate_link_token();
    diesel::insert_into(email_tokens::table)
        .values(&NewEmailToken {
            id: new_id(),
            user_id: user.id.clone(),
            purpose: purpose.into(),
            email: user.email.clone(),
            token_hash: auth::hash_refresh_token(&token),
            expires_at: later(lasts),
            used_at: None,
            created_at: now_iso(),
        })
        .execute(conn)?;
    Ok(token)
}

/// Spends a link: checks it is live, for this purpose, and still for the
/// address on the account, then marks it used.
fn redeem(conn: &mut Conn, token: &str, purpose: &str) -> ApiResult<User> {
    let row: EmailToken = email_tokens::table
        .filter(email_tokens::token_hash.eq(auth::hash_refresh_token(token.trim())))
        .filter(email_tokens::purpose.eq(purpose))
        .select(EmailToken::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::BadRequest("that link is not one we sent".into()))?;
    if row.used_at.is_some() {
        return Err(ApiError::BadRequest(
            "that link has already been used, or a newer one was sent".into(),
        ));
    }
    if row.expires_at.as_str() < now_iso().as_str() {
        return Err(ApiError::BadRequest(
            "that link has expired — ask for a new one".into(),
        ));
    }
    let user = support::find_user(conn, &row.user_id)?;
    // Proof of an address is proof of that address, not of whatever the
    // account says now.
    if user.email != row.email {
        return Err(ApiError::BadRequest(
            "that link was for a different address".into(),
        ));
    }
    diesel::update(email_tokens::table.find(&row.id))
        .set(email_tokens::used_at.eq(now_iso()))
        .execute(conn)?;
    Ok(user)
}

/// Mails a link that proves the address on the account.
pub async fn send_verification(state: &AppState, user: &User) -> ApiResult<()> {
    state
        .limiter
        .check(&user.email, Action::Mail, MAIL_PER_MINUTE)?;
    let token = blocking(&state.pool, {
        let user = user.clone();
        move |conn| mint(conn, &user, "verify", chrono::Duration::hours(VERIFY_HOURS))
    })
    .await?;
    state
        .mailer
        .send(Message {
            to: user.email.clone(),
            subject: "Confirm your address for Just Type Studio".into(),
            body: format!(
                "Hello {},\n\nConfirm this is your address, so drafts and books shared with it reach you:\n\n{}/verify?token={token}\n\nThe link lasts {VERIFY_HOURS} hours. If you did not sign up, ignore this and nothing happens.\n",
                user.display_name, state.config.public_url
            ),
            ..Default::default()        })
        .await
}

/// A code from the authenticator, or failing that an unused recovery code.
pub fn check_second_factor(conn: &mut Conn, user: &User, code: &str) -> ApiResult<()> {
    let Some(secret) = user.totp_secret.as_deref() else {
        return Err(ApiError::BadRequest(
            "two-factor sign-in is not set up".into(),
        ));
    };
    if let Some(step) = auth::totp::verify(
        secret,
        code,
        chrono::Utc::now().timestamp(),
        user.totp_last_step,
    ) {
        diesel::update(users::table.find(&user.id))
            .set(users::totp_last_step.eq(step))
            .execute(conn)?;
        return Ok(());
    }
    let digest = auth::hash_refresh_token(&normalize_recovery(code));
    let spent = diesel::update(
        recovery_codes::table
            .filter(recovery_codes::user_id.eq(&user.id))
            .filter(recovery_codes::code_hash.eq(&digest))
            .filter(recovery_codes::used_at.is_null()),
    )
    .set(recovery_codes::used_at.eq(now_iso()))
    .execute(conn)?;
    if spent > 0 {
        return Ok(());
    }
    Err(ApiError::Unauthorized("that code is not right".into()))
}

fn normalize_recovery(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Ends sessions — all of them, or all but one — along with every refresh
/// token they could have renewed from.
pub fn revoke_sessions(conn: &mut Conn, user_id: &str, except: Option<&str>) -> ApiResult<usize> {
    let now = now_iso();
    let mut doomed = sessions::table
        .filter(sessions::user_id.eq(user_id))
        .filter(sessions::revoked_at.is_null())
        .into_boxed();
    if let Some(keep) = except {
        doomed = doomed.filter(sessions::id.ne(keep));
    }
    let ids: Vec<String> = doomed.select(sessions::id).load(conn)?;
    diesel::update(sessions::table.filter(sessions::id.eq_any(&ids)))
        .set(sessions::revoked_at.eq(&now))
        .execute(conn)?;
    let mut tokens = refresh_tokens::table
        .filter(refresh_tokens::user_id.eq(user_id))
        .filter(refresh_tokens::revoked_at.is_null())
        .into_boxed();
    if let Some(keep) = except {
        tokens = tokens.filter(
            refresh_tokens::session_id
                .ne(keep)
                .or(refresh_tokens::session_id.is_null()),
        );
    }
    let token_ids: Vec<String> = tokens.select(refresh_tokens::id).load(conn)?;
    diesel::update(refresh_tokens::table.filter(refresh_tokens::id.eq_any(&token_ids)))
        .set(refresh_tokens::revoked_at.eq(&now))
        .execute(conn)?;
    Ok(ids.len())
}

// --- proving the address -------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/me/verification",
    tag = "account",
    responses(
        (status = 200, description = "Sent, or already proved", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn resend_verification(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Done>> {
    let found = blocking(&state.pool, move |conn| support::find_user(conn, &user.id)).await?;
    if found.email_verified_at.is_some() {
        return Err(ApiError::Conflict(
            "this address is already confirmed".into(),
        ));
    }
    send_verification(&state, &found).await?;
    Ok(Json(Done { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/auth/verify",
    tag = "account",
    request_body = TokenRequest,
    responses(
        (status = 200, description = "Address confirmed", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn verify(
    State(state): State<AppState>,
    Json(body): Json<TokenRequest>,
) -> ApiResult<Json<Done>> {
    let public_url = state.config.public_url.clone();
    blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let user = redeem(conn, &body.token, "verify")?;
            diesel::update(users::table.find(&user.id))
                .set(users::email_verified_at.eq(now_iso()))
                .execute(conn)?;
            welcome_once(conn, &user, &public_url)
        })
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

/// How long the queue keeps trying to deliver a welcome before giving up.
const WELCOME_GIVE_UP_DAYS: i64 = 3;

/// Queues the welcome email, the first time an address is proved and never
/// again. Call inside the transaction that records the proof, so the welcome
/// is queued if and only if the confirmation sticks.
///
/// Claimed with a conditional update rather than read-then-write, so two
/// confirmations racing each other still send one.
fn welcome_once(conn: &mut Conn, user: &User, public_url: &str) -> ApiResult<()> {
    let claimed = diesel::update(
        users::table
            .find(&user.id)
            .filter(users::welcomed_at.is_null()),
    )
    .set(users::welcomed_at.eq(now_iso()))
    .execute(conn)?;
    if claimed == 0 {
        return Ok(());
    }
    crate::mail::enqueue(
        conn,
        &welcome_message(public_url, &user.email, &user.display_name),
        chrono::Utc::now() + chrono::Duration::days(WELCOME_GIVE_UP_DAYS),
    )
}

/// The welcome: what the place is for, and where to begin. One message, sent
/// because they signed up, so it carries no unsubscribe link — and so it must
/// stay a welcome, never an advertisement.
pub fn welcome_message(public_url: &str, to: &str, name: &str) -> Message {
    let desk = format!("{public_url}/");
    let prompts = format!("{public_url}/prompts");
    let settings = format!("{public_url}/settings/general");
    let body = format!(
        "Hello {name},\n\n\
Your address is confirmed — welcome to Just Type Studio.\n\n\
It is a quiet place to write something long: a novel, a novella, a story. \
Start a book from your desk, empty or from a template that lays out the \
chapters for you, and write scene by scene. Everything saves as you go, \
and keeps working offline.\n\n\
Start your first book:\n{desk}\n\n\
If you want a reason to write every day, Daily Prompts sends one prompt a \
day. Answer it privately, or share it and read what others made of it:\n{prompts}\n\n\
The page itself — type, line length, focus mode, spelling — is yours to \
change in Settings, along with two-factor sign-in:\n{settings}\n\n\
Happy writing.\n"
    );
    let button = "display:inline-block;padding:10px 16px;background:#7a4f2a;color:#fff;text-decoration:none;border-radius:4px";
    let html = format!(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;color:#222;max-width:560px;margin:0 auto;padding:24px;line-height:1.5\">\
<p>Hello {name},</p>\
<p>Your address is confirmed — welcome to Just Type Studio.</p>\
<p>It is a quiet place to write something long: a novel, a novella, a story. \
Start a book from your desk, empty or from a template that lays out the chapters for you, \
and write scene by scene. Everything saves as you go, and keeps working offline.</p>\
<p><a href=\"{desk}\" style=\"{button}\">Start your first book</a></p>\
<p>If you want a reason to write every day, <a href=\"{prompts}\">Daily Prompts</a> sends one prompt a day. \
Answer it privately, or share it and read what others made of it.</p>\
<p>The page itself — type, line length, focus mode, spelling — is yours to change in \
<a href=\"{settings}\">Settings</a>, along with two-factor sign-in.</p>\
<p>Happy writing.</p></body></html>",
        name = crate::jobs::escape(name),
        desk = crate::jobs::escape(&desk),
        prompts = crate::jobs::escape(&prompts),
        settings = crate::jobs::escape(&settings),
    );
    Message {
        to: to.into(),
        subject: "Welcome to Just Type Studio".into(),
        body,
        html: Some(html),
        headers: Vec::new(),
    }
}

// --- getting back in -----------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/auth/password/forgot",
    tag = "account",
    request_body = ForgotRequest,
    responses(
        (status = 200, description = "Sent if there is such an account", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn forgot(
    State(state): State<AppState>,
    Json(body): Json<ForgotRequest>,
) -> ApiResult<Json<Done>> {
    let email = normalize_email(&body.email)?;
    // Counted per address whether or not it has an account, so the limit
    // cannot be used to find out which ones do.
    state.limiter.check(&email, Action::Mail, MAIL_PER_MINUTE)?;
    let found = blocking(&state.pool, {
        let email = email.clone();
        move |conn| {
            let user: Option<User> = users::table
                .filter(users::email.eq(&email))
                .select(User::as_select())
                .first(conn)
                .optional()?;
            match user {
                Some(user) => {
                    let token = mint(
                        conn,
                        &user,
                        "reset",
                        chrono::Duration::minutes(RESET_MINUTES),
                    )?;
                    Ok(Some((user, token)))
                }
                None => Ok(None),
            }
        }
    })
    .await?;
    if let Some((user, token)) = found {
        let sent = state
            .mailer
            .send(Message {
                to: user.email.clone(),
                subject: "Resetting your Just Type Studio password".into(),
                body: format!(
                    "Hello {},\n\nSomeone asked to reset the password on this account. If it was you:\n\n{}/reset?token={token}\n\nThe link lasts {RESET_MINUTES} minutes and works once. Using it signs out every device. If it was not you, ignore this; your password has not changed.\n",
                    user.display_name, state.config.public_url
                ),
                ..Default::default()            })
            .await;
        // The answer is the same either way; a failure is for the logs.
        if let Err(e) = sent {
            tracing::error!(error = %e, "could not send a reset mail");
        }
    }
    Ok(Json(Done { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/auth/password/reset",
    tag = "account",
    request_body = ResetRequest,
    responses(
        (status = 200, description = "Password changed; every device signed out", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reset(
    State(state): State<AppState>,
    Json(body): Json<ResetRequest>,
) -> ApiResult<Json<Done>> {
    check_password(&body.password)?;
    let hash = auth::password::hash(&body.password)?;
    let public_url = state.config.public_url.clone();
    blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let user = redeem(conn, &body.token, "reset")?;
            let now = now_iso();
            diesel::update(users::table.find(&user.id))
                .set((
                    users::password_hash.eq(&hash),
                    users::updated_at.eq(&now),
                    // The link reached them at this address, which proves it.
                    users::email_verified_at
                        .eq(user.email_verified_at.clone().unwrap_or(now.clone())),
                ))
                .execute(conn)?;
            // Proving the address this way is still the first time for some.
            welcome_once(conn, &user, &public_url)?;
            // Whoever had the old password is out, everywhere.
            revoke_sessions(conn, &user.id, None)?;
            super::audit::record(
                conn,
                Some(&user.id),
                None,
                AuditAction::PasswordReset,
                "by emailed link; every session signed out",
            )?;
            Ok(())
        })
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    /// A blank paragraph (double Enter) is a scene break, while writing a book
    /// and when turning text into a project.
    #[serde(default)]
    pub double_enter_splits_scenes: Option<bool>,
}

/// The writer's own settings, the ones every device reads from the account
/// (`GET /api/me`) rather than keeping locally.
#[utoipa::path(
    patch,
    path = "/api/me/settings",
    tag = "account",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "Saved", body = User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_settings(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateSettingsRequest>,
) -> ApiResult<Json<User>> {
    let updated = blocking(&state.pool, move |conn| {
        if let Some(on) = body.double_enter_splits_scenes {
            diesel::update(users::table.find(&user.id))
                .set(users::double_enter_splits_scenes.eq(on))
                .execute(conn)?;
        }
        support::find_user(conn, &user.id)
    })
    .await?;
    Ok(Json(updated))
}

#[utoipa::path(
    post,
    path = "/api/me/password",
    tag = "account",
    request_body = ChangePasswordRequest,
    responses(
        (status = 200, description = "Changed; other devices signed out", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ChangePasswordRequest>,
) -> ApiResult<Json<Done>> {
    check_password(&body.new_password)?;
    let hash = auth::password::hash(&body.new_password)?;
    blocking(&state.pool, move |conn| {
        let found = support::find_user(conn, &user.id)?;
        if !auth::password::verify(&body.current_password, &found.password_hash) {
            return Err(ApiError::Unauthorized(
                "that is not your current password".into(),
            ));
        }
        diesel::update(users::table.find(&user.id))
            .set((
                users::password_hash.eq(&hash),
                users::updated_at.eq(now_iso()),
            ))
            .execute(conn)?;
        // This device stays signed in; any other one has to know the new one.
        revoke_sessions(conn, &user.id, Some(&user.session_id))?;
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            AuditAction::PasswordChanged,
            "other sessions signed out",
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

// --- a second factor -----------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/me/two-factor/setup",
    tag = "account",
    responses(
        (status = 200, description = "A new key, not yet in force", body = TwoFactorSetup),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn setup_two_factor(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<TwoFactorSetup>> {
    let setup = blocking(&state.pool, move |conn| {
        let found = support::find_user(conn, &user.id)?;
        if found.totp_enabled_at.is_some() {
            return Err(ApiError::Conflict(
                "two-factor sign-in is already on — turn it off first to change the key".into(),
            ));
        }
        // Written now, in force only once a code from it has been checked.
        let secret = auth::totp::generate_secret();
        diesel::update(users::table.find(&user.id))
            .set((
                users::totp_secret.eq(&secret),
                users::totp_last_step.eq(None::<i64>),
            ))
            .execute(conn)?;
        Ok(TwoFactorSetup {
            url: auth::totp::setup_url(&secret, &found.email),
            secret,
        })
    })
    .await?;
    Ok(Json(setup))
}

#[utoipa::path(
    post,
    path = "/api/me/two-factor/enable",
    tag = "account",
    request_body = CodeRequest,
    responses(
        (status = 200, description = "On; the recovery codes, shown once", body = RecoveryCodes),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn enable_two_factor(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CodeRequest>,
) -> ApiResult<Json<RecoveryCodes>> {
    state.limiter.check(
        &user.id,
        Action::SecondFactor,
        state.config.second_factor_rate,
    )?;
    let codes = blocking(&state.pool, move |conn| {
        let found = support::find_user(conn, &user.id)?;
        if found.totp_enabled_at.is_some() {
            return Err(ApiError::Conflict(
                "two-factor sign-in is already on".into(),
            ));
        }
        let Some(secret) = found.totp_secret.as_deref() else {
            return Err(ApiError::BadRequest("start the setup first".into()));
        };
        // Proving the authenticator has the key is what turns it on, so a
        // mistyped key cannot lock anyone out.
        let step = auth::totp::verify(secret, &body.code, chrono::Utc::now().timestamp(), None)
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "that code does not match — check the key was entered exactly".into(),
                )
            })?;
        let codes: Vec<String> = (0..RECOVERY_CODES).map(|_| recovery_code()).collect();
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::update(users::table.find(&user.id))
                .set((
                    users::totp_enabled_at.eq(now_iso()),
                    users::totp_last_step.eq(step),
                ))
                .execute(conn)?;
            diesel::delete(recovery_codes::table.filter(recovery_codes::user_id.eq(&user.id)))
                .execute(conn)?;
            for code in &codes {
                diesel::insert_into(recovery_codes::table)
                    .values(&NewRecoveryCode {
                        id: new_id(),
                        user_id: user.id.clone(),
                        code_hash: auth::hash_refresh_token(&normalize_recovery(code)),
                        used_at: None,
                    })
                    .execute(conn)?;
            }
            super::audit::record(
                conn,
                Some(&user.id),
                None,
                AuditAction::TwoFactorEnabled,
                "",
            )?;
            Ok(())
        })?;
        Ok(codes)
    })
    .await?;
    Ok(Json(RecoveryCodes { codes }))
}

/// Ten random characters in two groups, from an alphabet without the ones
/// people misread.
fn recovery_code() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    let chars: String = (0..10)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    format!("{}-{}", &chars[..5], &chars[5..])
}

#[utoipa::path(
    post,
    path = "/api/me/two-factor/disable",
    tag = "account",
    request_body = DisableTwoFactorRequest,
    responses(
        (status = 200, description = "Off", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn disable_two_factor(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<DisableTwoFactorRequest>,
) -> ApiResult<Json<Done>> {
    state.limiter.check(
        &user.id,
        Action::SecondFactor,
        state.config.second_factor_rate,
    )?;
    blocking(&state.pool, move |conn| {
        let found = support::find_user(conn, &user.id)?;
        if found.totp_enabled_at.is_none() {
            return Err(ApiError::Conflict("two-factor sign-in is not on".into()));
        }
        // Both factors to remove one: a stolen session alone is not enough.
        if !auth::password::verify(&body.password, &found.password_hash) {
            return Err(ApiError::Unauthorized("that is not your password".into()));
        }
        check_second_factor(conn, &found, &body.code)?;
        diesel::update(users::table.find(&user.id))
            .set((
                users::totp_secret.eq(None::<String>),
                users::totp_enabled_at.eq(None::<String>),
                users::totp_last_step.eq(None::<i64>),
            ))
            .execute(conn)?;
        diesel::delete(recovery_codes::table.filter(recovery_codes::user_id.eq(&user.id)))
            .execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            AuditAction::TwoFactorDisabled,
            "",
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

// --- where it is signed in -----------------------------------------------

#[utoipa::path(
    get,
    path = "/api/me/sessions",
    tag = "account",
    responses(
        (status = 200, description = "Every device signed in, most recent first", body = Vec<SessionView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<SessionView>>> {
    let rows = blocking(&state.pool, move |conn| {
        let rows: Vec<Session> = sessions::table
            .filter(sessions::user_id.eq(&user.id))
            .filter(sessions::revoked_at.is_null())
            .order(sessions::last_seen_at.desc())
            .select(Session::as_select())
            .load(conn)?;
        Ok(rows
            .into_iter()
            .map(|session| SessionView {
                current: session.id == user.session_id,
                session,
            })
            .collect())
    })
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    delete,
    path = "/api/me/sessions/{session_id}",
    tag = "account",
    params(("session_id" = String, Path)),
    responses(
        (status = 200, description = "Signed out", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn revoke_session(
    State(state): State<AppState>,
    user: AuthUser,
    Path(session_id): Path<String>,
) -> ApiResult<Json<Done>> {
    blocking(&state.pool, move |conn| {
        let now = now_iso();
        let ended = diesel::update(
            sessions::table
                .find(&session_id)
                .filter(sessions::user_id.eq(&user.id))
                .filter(sessions::revoked_at.is_null()),
        )
        .set(sessions::revoked_at.eq(&now))
        .execute(conn)?;
        if ended == 0 {
            return Err(ApiError::NotFound("no such session".into()));
        }
        diesel::update(
            refresh_tokens::table
                .filter(refresh_tokens::session_id.eq(&session_id))
                .filter(refresh_tokens::revoked_at.is_null()),
        )
        .set(refresh_tokens::revoked_at.eq(&now))
        .execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            AuditAction::SessionRevoked,
            &session_id,
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/me/sessions/sign-out-others",
    tag = "account",
    responses(
        (status = 200, description = "Every other device signed out", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn sign_out_others(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Done>> {
    blocking(&state.pool, move |conn| {
        let ended = revoke_sessions(conn, &user.id, Some(&user.session_id))?;
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            AuditAction::SessionsRevoked,
            &format!("{ended} other session(s)"),
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(Done { ok: true }))
}
