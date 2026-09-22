//! Downloading a version as a file someone else can open.
//!
//! Exports run from a frozen snapshot, never the working copy: a file you send
//! an agent has to be a thing that stays the same, and the version is already
//! exactly that.
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use diesel::prelude::*;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{User, Work};
use crate::db::schema::users;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::export::{self, ChapterContent, Contact, Manuscript, SceneContent, Style};
use crate::state::AppState;

use super::support;
use super::versions::{self, VersionDetail};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    /// `docx`, `epub`, `pdf` or `txt`.
    pub format: String,
    /// `manuscript` (Shunn) or `book`. Ignored for EPUB, which has no pages,
    /// and for PDF, which is manuscript-only — see `export::pdf`.
    #[serde(default)]
    pub style: Option<String>,
}

fn style_from(value: Option<&str>) -> ApiResult<Style> {
    match value.unwrap_or("manuscript") {
        "manuscript" => Ok(Style::Manuscript),
        "book" => Ok(Style::Book),
        other => Err(ApiError::BadRequest(format!(
            "style must be `manuscript` or `book`, got `{other}`"
        ))),
    }
}

/// Everything after the last path separator, minus anything a filesystem or a
/// `Content-Disposition` header would rather not see.
fn safe_filename(title: &str, extension: &str) -> String {
    let mut stem = String::new();
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            stem.push(ch);
        } else if !stem.ends_with('-') {
            // Every run of punctuation or whitespace collapses to one
            // separator, so a title with a colon in it does not come out full
            // of `---`.
            stem.push('-');
        }
    }
    let stem = stem.trim_matches('-');
    let stem = if stem.is_empty() { "manuscript" } else { stem };
    format!("{stem}.{extension}")
}

/// Pulls the frozen text off disk and builds the model every writer shares.
async fn assemble(
    state: &AppState,
    detail: VersionDetail,
    work: Work,
    author: User,
) -> ApiResult<Manuscript> {
    let content = versions::assemble_content(state, detail).await?;

    // Scenes with no text are outline scaffolding — a template's empty beats.
    // Carrying them through would print a page of lone scene-break marks, and a
    // chapter that is nothing but scaffolding would get a title page with
    // nothing under it.
    let chapters: Vec<ChapterContent> = content
        .chapters
        .into_iter()
        .map(|chapter| ChapterContent {
            title: chapter.title,
            scenes: chapter
                .scenes
                .into_iter()
                .map(|scene| SceneContent {
                    title: scene.title,
                    blocks: export::blocks_from_doc(&scene.content),
                })
                .filter(|scene| !scene.blocks.is_empty())
                .collect(),
        })
        .filter(|chapter| !chapter.scenes.is_empty())
        .collect();

    let byline = work
        .pen_name
        .clone()
        .unwrap_or_else(|| author.display_name.clone());
    // The header wants a surname. The last whitespace-separated word is a guess,
    // and a wrong one for plenty of names — but it is the writer's own byline,
    // and they can override it by setting a short title.
    let surname = byline
        .split_whitespace()
        .last()
        .unwrap_or(&byline)
        .to_string();

    Ok(Manuscript {
        short_title: work
            .short_title
            .clone()
            .unwrap_or_else(|| work.title.clone()),
        title: work.title,
        subtitle: work.subtitle,
        byline,
        surname,
        contact: Contact {
            legal_name: author.legal_name,
            address: author.mailing_address,
            phone: author.phone,
            email: Some(author.email),
        },
        copyright_line: work.copyright_line,
        dedication: work.dedication,
        epigraph: work.epigraph,
        epigraph_source: work.epigraph_source,
        word_count: content.version.word_count,
        chapters,
    })
}

#[utoipa::path(
    get,
    path = "/api/versions/{version_id}/export",
    tag = "versions",
    params(("version_id" = String, Path)),
    responses(
        (status = 200, description = "The manuscript as a file", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export_version(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<impl IntoResponse> {
    let style = style_from(query.style.as_deref())?;

    let (detail, work, author) = blocking(&state.pool, move |conn| {
        let (version, work) = support::version_for_owner(conn, &version_id, &user)?;
        let author: User = users::table
            .find(&work.owner_id)
            .select(User::as_select())
            .first(conn)?;
        let detail = versions::load_detail(conn, &version.id)?;
        Ok((detail, work, author))
    })
    .await?;

    let manuscript = assemble(&state, detail, work, author).await?;
    if manuscript.is_empty() {
        return Err(ApiError::BadRequest(
            "there is nothing to export: this version has no text in it".into(),
        ));
    }

    let (bytes, mime, extension) = match query.format.as_str() {
        "docx" => (
            export::docx::write(&manuscript, style)?,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        ),
        "epub" => (
            export::epub::write(&manuscript)?,
            "application/epub+zip",
            "epub",
        ),
        "pdf" => (export::pdf::write(&manuscript), "application/pdf", "pdf"),
        "txt" => (
            export::text::write(&manuscript, style).into_bytes(),
            "text/plain; charset=utf-8",
            "txt",
        ),
        other => {
            return Err(ApiError::BadRequest(format!(
                "format must be one of docx, epub, pdf, txt — got `{other}`"
            )))
        }
    };

    let filename = safe_filename(&manuscript.title, extension);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|e| ApiError::internal(e.to_string()))?,
    );

    Ok((StatusCode::OK, headers, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_lose_anything_awkward() {
        assert_eq!(
            safe_filename("To the Lighthouse", "docx"),
            "To-the-Lighthouse.docx"
        );
        assert_eq!(safe_filename("A/B: \"quoted\"", "pdf"), "A-B-quoted.pdf");
        assert_eq!(safe_filename("", "epub"), "manuscript.epub");
        assert_eq!(safe_filename("...", "txt"), "manuscript.txt");
    }

    #[test]
    fn styles_parse_or_are_refused() {
        assert_eq!(style_from(None).unwrap(), Style::Manuscript);
        assert_eq!(style_from(Some("book")).unwrap(), Style::Book);
        assert!(style_from(Some("fancy")).is_err());
    }
}
