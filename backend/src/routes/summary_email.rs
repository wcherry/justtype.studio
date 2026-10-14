//! The summary email setting: none, daily or weekly, and the timezone it is
//! counted and sent in. What goes in it and when is `crate::summary_email`.
use axum::extract::{Query, State};
use axum::Json;
use chrono_tz::Tz;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::clock;
use crate::db::blocking;
use crate::db::schema::summary_email_settings;
use crate::db::Conn;
use crate::domain::enums::SummaryEmail;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SummaryEmailView {
    #[schema(value_type = crate::domain::enums::SummaryEmail)]
    pub often: String,
    /// The IANA timezone it is counted and sent in. Unset until first chosen.
    pub timezone: Option<String>,
    /// The local time it goes out, `HH:MM`.
    pub send_time: String,
    /// It can only be turned on for a confirmed address.
    pub email_verified: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSummaryEmailRequest {
    #[schema(value_type = crate::domain::enums::SummaryEmail)]
    pub often: String,
    /// The device's IANA timezone, such as `Europe/London`. Kept as it was
    /// when left out.
    #[serde(default)]
    pub timezone: Option<String>,
}

fn load(conn: &mut Conn, user: &AuthUser) -> ApiResult<SummaryEmailView> {
    let row: Option<(String, String)> = summary_email_settings::table
        .find(&user.id)
        .select((
            summary_email_settings::often,
            summary_email_settings::timezone,
        ))
        .first(conn)
        .optional()?;
    let (often, timezone) = match row {
        Some((often, tz)) => (often, Some(tz)),
        None => (SummaryEmail::None.as_str().to_string(), None),
    };
    Ok(SummaryEmailView {
        often,
        timezone,
        send_time: format!("{:02}:00", crate::summary_email::SEND_HOUR),
        email_verified: user.email_verified,
    })
}

#[utoipa::path(
    get,
    path = "/api/me/summary-email",
    tag = "account",
    responses(
        (status = 200, description = "How often the summary goes", body = SummaryEmailView),
    )
)]
pub async fn get_summary_email(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<SummaryEmailView>> {
    let view = blocking(&state.pool, move |conn| load(conn, &user)).await?;
    Ok(Json(view))
}

/// Daily, weekly or none. The first one goes at the next send time after this,
/// never for a morning that has already gone.
#[utoipa::path(
    patch,
    path = "/api/me/summary-email",
    tag = "account",
    request_body = UpdateSummaryEmailRequest,
    responses(
        (status = 200, description = "Saved", body = SummaryEmailView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_summary_email(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateSummaryEmailRequest>,
) -> ApiResult<Json<SummaryEmailView>> {
    if !SummaryEmail::accepts(&body.often) {
        return Err(ApiError::BadRequest(format!(
            "often must be one of {}",
            SummaryEmail::options()
        )));
    }
    if let Some(tz) = &body.timezone {
        tz.parse::<Tz>().map_err(|_| {
            ApiError::BadRequest(format!(
                "timezone must be an IANA name, such as Europe/London, not `{tz}`"
            ))
        })?;
    }
    if body.often != SummaryEmail::None.as_str() && !user.email_verified {
        return Err(ApiError::Forbidden(
            "confirm your address first: the summary email only goes to a confirmed one".into(),
        ));
    }
    let view = blocking(&state.pool, move |conn| {
        let now = clock::now_iso();
        let tz = body
            .timezone
            .clone()
            .unwrap_or_else(|| clock::prompt_tz().name().to_string());
        diesel::insert_into(summary_email_settings::table)
            .values((
                summary_email_settings::user_id.eq(&user.id),
                summary_email_settings::often.eq(&body.often),
                summary_email_settings::timezone.eq(&tz),
                summary_email_settings::token.eq(crate::auth::generate_link_token()),
                summary_email_settings::updated_at.eq(&now),
            ))
            .on_conflict(summary_email_settings::user_id)
            .do_update()
            .set((
                summary_email_settings::often.eq(&body.often),
                summary_email_settings::updated_at.eq(&now),
            ))
            .execute(conn)?;
        if let Some(tz) = &body.timezone {
            diesel::update(summary_email_settings::table.find(&user.id))
                .set(summary_email_settings::timezone.eq(tz))
                .execute(conn)?;
        }
        load(conn, &user)
    })
    .await?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SummaryUnsubscribeQuery {
    pub token: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Unsubscribed {
    pub unsubscribed: bool,
}

/// One click, signed in or not: the link in the summary email's footer
/// (through the web page) and mail clients' `List-Unsubscribe-Post` both land
/// here. Turns the summary email off and nothing else.
#[utoipa::path(
    post,
    path = "/api/summary-email/unsubscribe",
    tag = "account",
    params(SummaryUnsubscribeQuery),
    responses(
        (status = 200, description = "The summary email is off", body = Unsubscribed),
        (status = 404, description = "That link is not one of ours", body = ErrorEnvelope),
    )
)]
pub async fn unsubscribe(
    State(state): State<AppState>,
    Query(query): Query<SummaryUnsubscribeQuery>,
) -> ApiResult<Json<Unsubscribed>> {
    blocking(&state.pool, move |conn| {
        let changed = diesel::update(
            summary_email_settings::table.filter(summary_email_settings::token.eq(&query.token)),
        )
        .set((
            summary_email_settings::often.eq(SummaryEmail::None.as_str()),
            summary_email_settings::updated_at.eq(clock::now_iso()),
        ))
        .execute(conn)?;
        if changed == 0 {
            return Err(ApiError::NotFound(
                "that unsubscribe link is not one we sent".into(),
            ));
        }
        Ok(())
    })
    .await?;
    Ok(Json(Unsubscribed { unsubscribed: true }))
}
