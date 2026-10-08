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
    /// `docx`, `epub`, `pdf`, `html` or `txt`.
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
        "html" => (
            export::html::write(&manuscript).into_bytes(),
            "text/html; charset=utf-8",
            "html",
        ),
        "txt" => (
            export::text::write(&manuscript, style).into_bytes(),
            "text/plain; charset=utf-8",
            "txt",
        ),
        other => {
            return Err(ApiError::BadRequest(format!(
                "format must be one of docx, epub, pdf, html, txt — got `{other}`"
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

/// The whole review as one document the author can work from.
///
/// Built as a `Manuscript` so it goes through the same writers as the book
/// does — a review you can print, mark up on a train, and bring back is worth
/// more than a web page you scroll past. The chapters are the book's chapters;
/// under each scene sit the quoted passage and the notes on it.
async fn assemble_feedback(
    state: &AppState,
    publication: crate::db::models::Publication,
    detail: VersionDetail,
    work: Work,
    author: User,
    comments: Vec<(crate::db::models::Comment, String)>,
    reactions: Vec<(String, i32, String, i64)>,
) -> ApiResult<Manuscript> {
    use crate::export::{Block, Span};

    let content = versions::assemble_content(state, detail).await?;
    let plain = |text: &str| {
        vec![Span {
            text: text.to_string(),
            ..Default::default()
        }]
    };

    let chapters: Vec<ChapterContent> = content
        .chapters
        .into_iter()
        .map(|chapter| ChapterContent {
            title: chapter.title,
            scenes: chapter
                .scenes
                .into_iter()
                .map(|scene| {
                    let mut blocks: Vec<Block> = Vec::new();

                    for (comment, author_name) in comments
                        .iter()
                        .filter(|(c, _)| c.scene_id.as_deref() == Some(scene.scene_id.as_str()))
                    {
                        if let Some(quote) = comment.anchor_quote.as_deref() {
                            blocks.push(Block::Quote(plain(quote)));
                        }
                        let mark = if comment.state == "resolved" {
                            " (resolved)"
                        } else {
                            ""
                        };
                        blocks.push(Block::Paragraph(plain(&format!(
                            "{author_name} — {}{mark}: {}",
                            comment.category, comment.body
                        ))));
                    }

                    let tallies: Vec<String> = reactions
                        .iter()
                        .filter(|(scene_id, ..)| scene_id == &scene.scene_id)
                        .map(|(_, block_index, kind, count)| {
                            format!("paragraph {}: {count} {kind}", block_index + 1)
                        })
                        .collect();
                    if !tallies.is_empty() {
                        blocks.push(Block::Paragraph(plain(&format!(
                            "Reactions — {}",
                            tallies.join("; ")
                        ))));
                    }

                    // A heading only where there is something under it, so an
                    // untouched scene does not take up a page saying nothing.
                    if !blocks.is_empty() {
                        blocks.insert(
                            0,
                            Block::Heading {
                                level: 2,
                                spans: plain(&scene.title),
                            },
                        );
                    }
                    SceneContent {
                        title: scene.title,
                        blocks,
                    }
                })
                .filter(|scene| !scene.blocks.is_empty())
                .collect(),
        })
        .filter(|chapter| !chapter.scenes.is_empty())
        .collect();

    let byline = author.display_name.clone();
    let surname = byline
        .split_whitespace()
        .last()
        .unwrap_or(&byline)
        .to_string();
    let title = format!("{} — feedback", work.title);
    Ok(Manuscript {
        short_title: title.clone(),
        title,
        subtitle: Some(format!(
            "{} · {} note{}",
            publication.created_at.split('T').next().unwrap_or_default(),
            comments.len(),
            if comments.len() == 1 { "" } else { "s" }
        )),
        byline,
        surname,
        contact: Contact::default(),
        copyright_line: None,
        dedication: None,
        epigraph: None,
        epigraph_source: None,
        word_count: 0,
        chapters,
    })
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/feedback/export",
    tag = "publications",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "The review as a file", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
/// Every note on a publication, as a document. The author's only: it carries
/// everyone's feedback with their names on it.
pub async fn export_feedback(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<impl IntoResponse> {
    let style = style_from(query.style.as_deref())?;

    let (publication, detail, work, author, comments, reactions) =
        blocking(&state.pool, move |conn| {
            use crate::db::schema::{comments, reactions};
            let (publication, work) = support::publication_for_owner(conn, &publication_id, &user)?;
            let author: User = users::table
                .find(&work.owner_id)
                .select(User::as_select())
                .first(conn)?;
            let detail = versions::load_detail(conn, &publication.version_id)?;

            let rows: Vec<crate::db::models::Comment> = comments::table
                .filter(comments::publication_id.eq(&publication.id))
                .filter(comments::state.ne("deleted"))
                .order(comments::created_at.asc())
                .select(crate::db::models::Comment::as_select())
                .load(conn)?;
            let ids: Vec<String> = rows.iter().map(|c| c.author_id.clone()).collect();
            let cards = support::author_cards(conn, &ids)?;
            let with_names = rows
                .into_iter()
                .map(|c| {
                    let name = cards
                        .get(&c.author_id)
                        .map(|a| a.display_name.clone())
                        .unwrap_or_else(|| "Unknown".into());
                    (c, name)
                })
                .collect();

            let tallies: Vec<(String, i32, String, i64)> = reactions::table
                .filter(reactions::publication_id.eq(&publication.id))
                .group_by((reactions::scene_id, reactions::block_index, reactions::kind))
                .order((reactions::scene_id.asc(), reactions::block_index.asc()))
                .select((
                    reactions::scene_id,
                    reactions::block_index,
                    reactions::kind,
                    diesel::dsl::count_star(),
                ))
                .load(conn)?;

            Ok((publication, detail, work, author, with_names, tallies))
        })
        .await?;

    let manuscript = assemble_feedback(
        &state,
        publication,
        detail,
        work,
        author,
        comments,
        reactions,
    )
    .await?;
    if manuscript.is_empty() {
        return Err(ApiError::BadRequest(
            "there is nothing to export: nobody has said anything yet".into(),
        ));
    }

    let (bytes, mime, extension) = match query.format.as_str() {
        "docx" => (
            export::docx::write(&manuscript, style)?,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        ),
        "pdf" => (export::pdf::write(&manuscript), "application/pdf", "pdf"),
        "txt" => (
            export::text::write(&manuscript, style).into_bytes(),
            "text/plain; charset=utf-8",
            "txt",
        ),
        other => {
            return Err(ApiError::BadRequest(format!(
                "format must be one of docx, pdf, txt — got `{other}`"
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
