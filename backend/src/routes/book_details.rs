//! What a book says about itself beyond its title and byline: language,
//! description, contributors, publisher, an identifier, keywords and a cover.
//!
//! These live in their own resource rather than on the work, and every field
//! is saved by the one handler below — an earlier bug had front-matter fields
//! silently dropped because a second handler listed them separately.
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::WorkMetadata;
use crate::db::schema::work_metadata;
use crate::domain::now_iso;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support;

/// A cover is a single image; more than this is a mistake, not a cover.
pub const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;
const MIN_COVER_SIDE: u32 = 1600;
const MIN_COVER_LONG_SIDE: u32 = 2560;
const ROLES: &[&str] = &["editor", "translator", "illustrator", "co-author"];

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContributorBody {
    pub name: String,
    /// `editor`, `translator`, `illustrator` or `co-author`.
    pub role: String,
}

/// Each field is sent only when it changes; an empty string (or list) clears
/// it, which is what a writer emptying the field in the UI means.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadataRequest {
    /// A BCP 47 tag: `en`, `fr-CA`, `he`.
    #[serde(default)]
    pub language: Option<String>,
    /// `ltr` or `rtl`. Empty follows the language.
    #[serde(default)]
    pub text_direction: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    #[serde(default)]
    pub published: Option<String>,
    /// An ISBN or any other identifier. Optional; never required.
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub contributors: Option<Vec<ContributorBody>>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetadataResponse {
    pub language: Option<String>,
    pub text_direction: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub published: Option<String>,
    pub identifier: Option<String>,
    pub keywords: Vec<String>,
    pub contributors: Vec<ContributorBody>,
    pub has_cover: bool,
    pub cover_bytes: Option<i32>,
    pub cover_width: Option<i32>,
    pub cover_height: Option<i32>,
    /// Set when the cover is smaller than e-readers like.
    pub cover_warning: Option<String>,
    pub updated_at: Option<String>,
}

fn respond(row: Option<WorkMetadata>) -> MetadataResponse {
    let Some(row) = row else {
        return MetadataResponse {
            language: None,
            text_direction: None,
            description: None,
            publisher: None,
            published: None,
            identifier: None,
            keywords: Vec::new(),
            contributors: Vec::new(),
            has_cover: false,
            cover_bytes: None,
            cover_width: None,
            cover_height: None,
            cover_warning: None,
            updated_at: None,
        };
    };
    let cover_warning = match (row.cover_width, row.cover_height) {
        (Some(w), Some(h)) if cover_too_small(w as u32, h as u32) => Some(format!(
            "This cover is {w}×{h}. At least {MIN_COVER_SIDE}×{MIN_COVER_LONG_SIDE} looks \
             sharp on current e-readers."
        )),
        _ => None,
    };
    MetadataResponse {
        language: row.language,
        text_direction: row.text_direction,
        description: row.description,
        publisher: row.publisher,
        published: row.published,
        identifier: row.identifier,
        keywords: row
            .keywords
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default(),
        contributors: row
            .contributors
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default(),
        has_cover: row.cover_mime.is_some(),
        cover_bytes: row.cover_bytes,
        cover_width: row.cover_width,
        cover_height: row.cover_height,
        cover_warning,
        updated_at: Some(row.updated_at),
    }
}

fn cover_too_small(w: u32, h: u32) -> bool {
    let (short, long) = if w < h { (w, h) } else { (h, w) };
    short < MIN_COVER_SIDE || long < MIN_COVER_LONG_SIDE
}

fn clean(value: &str, limit: usize, what: &str) -> ApiResult<Option<String>> {
    let trimmed = value.trim();
    if trimmed.chars().count() > limit {
        return Err(ApiError::BadRequest(format!(
            "{what} must be at most {limit} characters"
        )));
    }
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

/// A language tag in the shape BCP 47 uses. Not a registry lookup: the point
/// is to stop `english` or a sentence from ending up in `dc:language`.
pub fn valid_language(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let Some(primary) = parts.next() else {
        return false;
    };
    (2..=3).contains(&primary.len())
        && primary.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| {
            (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

fn valid_date(date: &str) -> bool {
    let parts: Vec<&str> = date.split('-').collect();
    let number = |s: &str, len: usize| s.len() == len && s.chars().all(|c| c.is_ascii_digit());
    match parts.as_slice() {
        [y] => number(y, 4),
        [y, m] => number(y, 4) && number(m, 2) && (1..=12).contains(&m.parse::<u32>().unwrap_or(0)),
        [y, m, d] => {
            number(y, 4)
                && number(m, 2)
                && number(d, 2)
                && chrono::NaiveDate::from_ymd_opt(
                    y.parse().unwrap_or(0),
                    m.parse().unwrap_or(0),
                    d.parse().unwrap_or(0),
                )
                .is_some()
        }
        _ => false,
    }
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/metadata",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The book's details", body = MetadataResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_metadata(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<MetadataResponse>> {
    let row = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        Ok(work_metadata::table
            .find(&work_id)
            .select(WorkMetadata::as_select())
            .first(conn)
            .optional()?)
    })
    .await?;
    Ok(Json(respond(row)))
}

#[utoipa::path(
    patch,
    path = "/api/works/{work_id}/metadata",
    tag = "works",
    params(("work_id" = String, Path)),
    request_body = UpdateMetadataRequest,
    responses(
        (status = 200, description = "Saved", body = MetadataResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_metadata(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<UpdateMetadataRequest>,
) -> ApiResult<Json<MetadataResponse>> {
    // Checked before touching the database, so a refusal changes nothing.
    let language = body
        .language
        .as_deref()
        .map(|v| clean(v, 35, "language"))
        .transpose()?;
    if let Some(Some(tag)) = &language {
        if !valid_language(tag) {
            return Err(ApiError::BadRequest(format!(
                "`{tag}` is not a language tag. Use a short code such as `en`, `fr` or `pt-BR`."
            )));
        }
    }
    let direction = body
        .text_direction
        .as_deref()
        .map(|v| clean(v, 3, "text direction"))
        .transpose()?;
    if let Some(Some(d)) = &direction {
        if d != "ltr" && d != "rtl" {
            return Err(ApiError::BadRequest(
                "text direction must be `ltr` or `rtl`, or empty to follow the language".into(),
            ));
        }
    }
    let description = body
        .description
        .as_deref()
        .map(|v| clean(v, 4000, "description"))
        .transpose()?;
    let publisher = body
        .publisher
        .as_deref()
        .map(|v| clean(v, 200, "publisher"))
        .transpose()?;
    let identifier = body
        .identifier
        .as_deref()
        .map(|v| clean(v, 100, "identifier"))
        .transpose()?;
    let published = body
        .published
        .as_deref()
        .map(|v| clean(v, 10, "publication date"))
        .transpose()?;
    if let Some(Some(date)) = &published {
        if !valid_date(date) {
            return Err(ApiError::BadRequest(
                "the publication date must be a year, a month (2026-03) or a day (2026-03-01)"
                    .into(),
            ));
        }
    }
    let keywords = match &body.keywords {
        None => None,
        Some(list) => {
            if list.len() > 30 {
                return Err(ApiError::BadRequest("at most 30 keywords".into()));
            }
            let mut out = Vec::new();
            for keyword in list {
                if let Some(k) = clean(keyword, 60, "a keyword")? {
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            }
            Some(serde_json::to_string(&out).expect("strings serialise"))
        }
    };
    let contributors = match &body.contributors {
        None => None,
        Some(list) => {
            if list.len() > 20 {
                return Err(ApiError::BadRequest("at most 20 contributors".into()));
            }
            let mut out = Vec::new();
            for c in list {
                let Some(name) = clean(&c.name, 120, "a contributor's name")? else {
                    continue;
                };
                if !ROLES.contains(&c.role.as_str()) {
                    return Err(ApiError::BadRequest(format!(
                        "a contributor's role must be one of {}",
                        ROLES.join(", ")
                    )));
                }
                out.push(ContributorBody {
                    name,
                    role: c.role.clone(),
                });
            }
            Some(serde_json::to_string(&out).expect("contributors serialise"))
        }
    };

    let row = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        let now = now_iso();
        diesel::insert_or_ignore_into(work_metadata::table)
            .values((
                work_metadata::work_id.eq(&work_id),
                work_metadata::updated_at.eq(&now),
            ))
            .execute(conn)?;
        // One field at a time, so a field not sent is a field not touched.
        macro_rules! set {
            ($value:expr, $column:ident) => {
                if let Some(value) = $value {
                    diesel::update(work_metadata::table.find(&work_id))
                        .set(work_metadata::$column.eq(value))
                        .execute(conn)?;
                }
            };
        }
        set!(language, language);
        set!(direction, text_direction);
        set!(description, description);
        set!(publisher, publisher);
        set!(published, published);
        set!(identifier, identifier);
        set!(keywords.map(Some), keywords);
        set!(contributors.map(Some), contributors);
        diesel::update(work_metadata::table.find(&work_id))
            .set(work_metadata::updated_at.eq(&now))
            .execute(conn)?;
        Ok(work_metadata::table
            .find(&work_id)
            .select(WorkMetadata::as_select())
            .first(conn)
            .optional()?)
    })
    .await?;
    Ok(Json(respond(row)))
}

/// What the first bytes say an image is, and how big: PNG's header or the
/// first JPEG frame marker. Anything else is not a cover.
pub fn sniff_image(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() > 24 && bytes.starts_with(PNG) && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some(("image/png", w, h));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            // Fill bytes and markers with no length.
            if marker == 0xFF || (0xD0..=0xD8).contains(&marker) || marker == 0x01 {
                i += if marker == 0xFF { 1 } else { 2 };
                continue;
            }
            let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            // Start-of-frame markers, bar the three that are not frames.
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some(("image/jpeg", w, h));
            }
            i += 2 + len;
        }
    }
    None
}

#[utoipa::path(
    put,
    path = "/api/works/{work_id}/cover",
    tag = "works",
    params(("work_id" = String, Path)),
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "A JPEG or PNG image"),
    responses(
        (status = 200, description = "Saved", body = MetadataResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn put_cover(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<MetadataResponse>> {
    if body.is_empty() {
        return Err(ApiError::BadRequest("send the image as the request body".into()));
    }
    if body.len() > MAX_COVER_BYTES {
        return Err(ApiError::BadRequest(format!(
            "a cover can be at most {} MB. Export a smaller JPEG.",
            MAX_COVER_BYTES / 1024 / 1024
        )));
    }
    let Some((mime, width, height)) = sniff_image(&body) else {
        return Err(ApiError::BadRequest(
            "that is not a JPEG or PNG image. Save the cover as one of those and try again."
                .into(),
        ));
    };
    if width < 200 || height < 200 || width > 12_000 || height > 12_000 {
        return Err(ApiError::BadRequest(format!(
            "this cover is {width}×{height}; it must be between 200 and 12000 pixels on each side"
        )));
    }

    // Owner check first, so a stranger cannot write to the disk.
    {
        let work_id = work_id.clone();
        let user = user.clone();
        blocking(&state.pool, move |conn| {
            support::work_for_owner(conn, &work_id, &user)?;
            Ok(())
        })
        .await?;
    }
    state.store.write_cover(&work_id, &body).await?;
    let size = body.len() as i32;
    let row = blocking(&state.pool, move |conn| {
        let now = now_iso();
        diesel::insert_or_ignore_into(work_metadata::table)
            .values((
                work_metadata::work_id.eq(&work_id),
                work_metadata::updated_at.eq(&now),
            ))
            .execute(conn)?;
        diesel::update(work_metadata::table.find(&work_id))
            .set((
                work_metadata::cover_mime.eq(Some(mime)),
                work_metadata::cover_bytes.eq(Some(size)),
                work_metadata::cover_width.eq(Some(width as i32)),
                work_metadata::cover_height.eq(Some(height as i32)),
                work_metadata::updated_at.eq(&now),
            ))
            .execute(conn)?;
        Ok(work_metadata::table
            .find(&work_id)
            .select(WorkMetadata::as_select())
            .first(conn)
            .optional()?)
    })
    .await?;
    Ok(Json(respond(row)))
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/cover",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The cover image", content_type = "image/*"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_cover(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let id = work_id.clone();
    let mime = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &id, &user)?;
        Ok(work_metadata::table
            .find(&id)
            .select(work_metadata::cover_mime)
            .first::<Option<String>>(conn)
            .optional()?
            .flatten())
    })
    .await?;
    let Some(mime) = mime else {
        return Err(ApiError::NotFound("this book has no cover".into()));
    };
    let bytes = state
        .store
        .read_cover(&work_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("this book has no cover".into()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&mime).map_err(|e| ApiError::internal(e.to_string()))?,
    );
    // Private to the owner, and the same URL serves a replacement.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    Ok((StatusCode::OK, headers, bytes))
}

#[utoipa::path(
    delete,
    path = "/api/works/{work_id}/cover",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Removed", body = MetadataResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_cover(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<MetadataResponse>> {
    let id = work_id.clone();
    blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &id, &user)?;
        Ok(())
    })
    .await?;
    state.store.delete_cover(&work_id).await?;
    let row = blocking(&state.pool, move |conn| {
        diesel::update(work_metadata::table.find(&work_id))
            .set((
                work_metadata::cover_mime.eq(None::<String>),
                work_metadata::cover_bytes.eq(None::<i32>),
                work_metadata::cover_width.eq(None::<i32>),
                work_metadata::cover_height.eq(None::<i32>),
                work_metadata::updated_at.eq(now_iso()),
            ))
            .execute(conn)?;
        Ok(work_metadata::table
            .find(&work_id)
            .select(WorkMetadata::as_select())
            .first(conn)
            .optional()?)
    })
    .await?;
    Ok(Json(respond(row)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0]);
        b
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        // SOI, an APP0 segment to skip, then a baseline frame header.
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        b.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);
        b
    }

    #[test]
    fn images_are_recognised_by_their_bytes_and_measured() {
        assert_eq!(sniff_image(&png(1600, 2560)), Some(("image/png", 1600, 2560)));
        assert_eq!(sniff_image(&jpeg(800, 1200)), Some(("image/jpeg", 800, 1200)));
        assert_eq!(sniff_image(b"GIF89a....................."), None);
        assert_eq!(sniff_image(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(sniff_image(&[]), None);
    }

    #[test]
    fn small_covers_are_flagged_not_refused() {
        assert!(cover_too_small(800, 1200));
        assert!(!cover_too_small(1600, 2560));
        assert!(!cover_too_small(2560, 1600));
    }

    #[test]
    fn language_tags_and_dates_are_checked() {
        for ok in ["en", "fr", "pt-BR", "zh-Hant-TW", "haw"] {
            assert!(valid_language(ok), "{ok}");
        }
        for bad in ["english", "e", "", "en_US", "en-", "a very long sentence"] {
            assert!(!valid_language(bad), "{bad}");
        }
        for ok in ["2026", "2026-03", "2026-03-01"] {
            assert!(valid_date(ok), "{ok}");
        }
        for bad in ["26", "2026-13", "2026-02-30", "March 2026", "2026-3-1"] {
            assert!(!valid_date(bad), "{bad}");
        }
    }
}
