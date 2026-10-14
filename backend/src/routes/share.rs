//! Signed, expiring links for unlisted publications.
//!
//! "Unlisted" used to mean "anyone who has the id": a link that worked forever
//! and could not be taken back short of withdrawing the publication. Now an
//! unlisted publication opens only with a key the author made:
//! `<expiry>.<signature>`, where the signature is an HMAC over the
//! publication, the expiry and a secret kept on the publication. Past its
//! expiry a key is refused; replacing the secret refuses every key at once.
//!
//! The key travels in an `x-share-key` header, never a query string after the
//! first page load, for the same reason tokens do: URLs are logged.
use std::sync::OnceLock;

use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::Publication;
use crate::db::schema::publications;
use crate::domain::enums::AuditAction;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support;

tokio::task_local! {
    /// The share keys this request carried, from `x-share-key`.
    pub static PRESENTED: Vec<String>;
}

static SIGNING_KEY: OnceLock<Vec<u8>> = OnceLock::new();

/// Derives the signing key from the server's secret, once at startup. A
/// separate key from the one that signs access tokens, so neither can stand
/// in for the other.
pub fn configure(server_secret: &str) {
    let mut mac = Hmac::<Sha256>::new_from_slice(server_secret.as_bytes())
        .expect("HMAC takes any key length");
    mac.update(b"scriptorium share links v1");
    let _ = SIGNING_KEY.set(mac.finalize().into_bytes().to_vec());
}

const MAX_DAYS: i64 = 90;

fn mac(publication_id: &str, expires: i64, secret: &str) -> Hmac<Sha256> {
    let key = SIGNING_KEY
        .get()
        .map(Vec::as_slice)
        .unwrap_or(b"unconfigured");
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(publication_id.as_bytes());
    mac.update(b"\0");
    mac.update(expires.to_string().as_bytes());
    mac.update(b"\0");
    mac.update(secret.as_bytes());
    mac
}

pub fn sign(publication_id: &str, expires: i64, secret: &str) -> String {
    let tag = mac(publication_id, expires, secret).finalize().into_bytes();
    format!("{expires}.{}", hex::encode(tag))
}

/// Whether a key opens this publication now. Compared in constant time.
pub fn valid(publication: &Publication, key: &str, now: i64) -> bool {
    let Some(secret) = publication.link_secret.as_deref() else {
        return false;
    };
    let Some((expires, signature)) = key.split_once('.') else {
        return false;
    };
    let Ok(expires) = expires.parse::<i64>() else {
        return false;
    };
    let Ok(signature) = hex::decode(signature) else {
        return false;
    };
    expires > now
        && mac(&publication.id, expires, secret)
            .verify_slice(&signature)
            .is_ok()
}

/// Whether the current request carries a key that opens this publication.
pub fn presented_for(publication: &Publication) -> bool {
    let now = chrono::Utc::now().timestamp();
    PRESENTED
        .try_with(|keys| keys.iter().any(|k| valid(publication, k, now)))
        .unwrap_or(false)
}

/// Reads `x-share-key` (several, comma-separated) for `presented_for`.
pub fn keys_from(headers: &axum::http::HeaderMap) -> Vec<String> {
    headers
        .get("x-share-key")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty() && k.len() <= 100)
                .take(10)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MakeLinkRequest {
    /// How long it works, 1 to 90 days.
    pub days: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareLink {
    /// The whole link, to send to someone.
    pub url: String,
    pub key: String,
    pub expires_at: String,
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/links",
    tag = "publications",
    params(("publication_id" = String, Path)),
    request_body = MakeLinkRequest,
    responses(
        (status = 200, description = "A link that works until it expires", body = ShareLink),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn make(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<MakeLinkRequest>,
) -> ApiResult<Json<ShareLink>> {
    if !(1..=MAX_DAYS).contains(&body.days) {
        return Err(ApiError::BadRequest(format!(
            "a link lasts between 1 and {MAX_DAYS} days"
        )));
    }
    let (publication, secret) = blocking(&state.pool, move |conn| {
        let (publication, work) = support::publication_for_owner(conn, &publication_id, &user)?;
        if publication.visibility != "unlisted" {
            return Err(ApiError::BadRequest(
                "only an unlisted publication is opened by link — the others open by who is asking"
                    .into(),
            ));
        }
        // Publications from before links were signed get their secret now.
        let secret = match &publication.link_secret {
            Some(s) => s.clone(),
            None => {
                let fresh = crate::auth::generate_link_token();
                diesel::update(publications::table.find(&publication.id))
                    .set(publications::link_secret.eq(&fresh))
                    .execute(conn)?;
                fresh
            }
        };
        super::audit::record(
            conn,
            Some(&user.id),
            Some(&work.id),
            AuditAction::ShareLinkMade,
            &format!("{}: {} day(s)", publication.id, body.days),
        )?;
        Ok((publication, secret))
    })
    .await?;
    let expires = chrono::Utc::now() + chrono::Duration::days(body.days);
    let key = sign(&publication.id, expires.timestamp(), &secret);
    Ok(Json(ShareLink {
        url: format!(
            "{}/review/{}?key={key}",
            state.config.public_url, publication.id
        ),
        key,
        expires_at: expires.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }))
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/links/revoke",
    tag = "publications",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Every link made so far stops working", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    blocking(&state.pool, move |conn| {
        let (publication, work) = support::publication_for_owner(conn, &publication_id, &user)?;
        diesel::update(publications::table.find(&publication.id))
            .set(publications::link_secret.eq(crate::auth::generate_link_token()))
            .execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            Some(&work.id),
            AuditAction::ShareLinksRevoked,
            &publication.id,
        )
    })
    .await?;
    Ok(Json(serde_json::json!({ "revoked": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn publication(secret: Option<&str>) -> Publication {
        Publication {
            id: "p1".into(),
            work_id: "w".into(),
            version_id: "v".into(),
            visibility: "unlisted".into(),
            scope: "work".into(),
            scope_ref_id: None,
            review_prompt: None,
            opens_at: None,
            closes_at: None,
            created_at: "t".into(),
            updated_at: "t".into(),
            max_reviewers: None,
            spoiler_safe: 0,
            link_secret: secret.map(str::to_string),
            revoked_at: None,
        }
    }

    #[test]
    fn a_key_opens_its_publication_until_it_expires_or_is_revoked() {
        configure("test-secret-value-1234567890");
        let p = publication(Some("s1"));
        let key = sign("p1", 2_000, "s1");
        assert!(valid(&p, &key, 1_999));
        assert!(!valid(&p, &key, 2_000), "expired");
        // Another publication, a revoked secret, a forged expiry, junk.
        assert!(!valid(
            &Publication {
                id: "p2".into(),
                ..p.clone()
            },
            &key,
            1_000
        ));
        assert!(!valid(&publication(Some("s2")), &key, 1_000));
        let forged = key.replacen("2000", "9999", 1);
        assert!(!valid(&p, &forged, 1_000));
        assert!(!valid(&p, "not-a-key", 1_000));
        assert!(!valid(&publication(None), &key, 1_000));
    }

    #[test]
    fn keys_come_from_one_header_and_are_bounded() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-share-key", "a.b, c.d ,".parse().unwrap());
        assert_eq!(keys_from(&headers), ["a.b", "c.d"]);
    }
}
