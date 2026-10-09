//! Downloading the book as a file someone else can open.
//!
//! A version is the export to send: a file you send an agent has to be a thing
//! that stays the same, and a version is already exactly that. The working
//! draft can be exported too, as it reads at this moment.
//!
//! The settings arrive as [`ExportOptions`]. The `GET ?format=&style=` forms
//! stay as shorthands that build the default options, so the iOS app and the
//! smoke test keep working.
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use diesel::prelude::*;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{User, Work, WorkMetadata};
use crate::db::schema::{annotations, users, work_metadata};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::export::assemble::{self, PreflightReport, Rendered, Resolved};
use crate::export::options::{ExportOptions, Format};
use crate::export::pipeline::{SourceChapter, SourceComment, SourceScene};
use crate::export::{self, ChapterContent, Contact, Cover, Manuscript, Role, SceneContent, Style};
use crate::state::AppState;

use super::support;
use super::versions::{self, VersionDetail};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportQuery {
    /// `docx`, `epub`, `pdf`, `html` or `txt` — and, for the working draft only,
    /// `project`: the whole book as a zip.
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

fn shorthand(query: &ExportQuery) -> ApiResult<ExportOptions> {
    let style = style_from(query.style.as_deref())?;
    let format = Format::parse(&query.format).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "format must be one of docx, epub, pdf, html, txt — got `{}`",
            query.format
        ))
    })?;
    Ok(ExportOptions::shorthand(format, style))
}

fn shorthand_for(query: &ExportQuery) -> ApiResult<ExportOptions> {
    shorthand(query)
}

pub(super) fn checked(options: ExportOptions) -> ApiResult<ExportOptions> {
    options.validate().map_err(ApiError::BadRequest)?;
    Ok(options)
}

pub(super) use crate::export::assemble::safe_filename;

/// The work's own details beyond the byline, and the writer's own comments.
/// Comments are private to whoever wrote them, so the query is by the person
/// exporting and nobody else.
fn load_extras(
    conn: &mut crate::db::Conn,
    work: &Work,
    user: &AuthUser,
) -> ApiResult<(Option<WorkMetadata>, Vec<SourceComment>)> {
    let meta = work_metadata::table
        .find(&work.id)
        .select(WorkMetadata::as_select())
        .first(conn)
        .optional()?;
    let rows: Vec<crate::db::models::Annotation> = annotations::table
        .filter(annotations::user_id.eq(&user.id))
        .filter(annotations::work_id.eq(&work.id))
        .filter(annotations::publication_id.is_null())
        .filter(annotations::kind.eq("comment"))
        .order(annotations::created_at.asc())
        .select(crate::db::models::Annotation::as_select())
        .load(conn)?;
    let comments = rows
        .into_iter()
        .filter_map(|a| {
            Some(SourceComment {
                scene_id: a.scene_id,
                from: a.from_pos,
                to: a.to_pos,
                quote: a.quote,
                note: a.note?,
            })
        })
        .collect();
    Ok((meta, comments))
}

async fn load_cover(
    state: &AppState,
    work_id: &str,
    meta: &Option<WorkMetadata>,
) -> ApiResult<Option<Cover>> {
    let Some(mime) = meta.as_ref().and_then(|m| m.cover_mime.clone()) else {
        return Ok(None);
    };
    Ok(state
        .store
        .read_cover(work_id)
        .await?
        .map(|bytes| Cover { bytes, mime }))
}

/// Stage 0, for a frozen version: pull the text off disk.
pub(super) async fn resolve_version(
    state: &AppState,
    user: AuthUser,
    version_id: String,
) -> ApiResult<Resolved> {
    let (detail, work, author, meta, comments) = blocking(&state.pool, move |conn| {
        let (version, work) = support::version_for_owner(conn, &version_id, &user)?;
        let author: User = users::table
            .find(&work.owner_id)
            .select(User::as_select())
            .first(conn)?;
        let detail = versions::load_detail(conn, &version.id)?;
        let (meta, comments) = load_extras(conn, &work, &user)?;
        Ok((detail, work, author, meta, comments))
    })
    .await?;
    let cover = load_cover(state, &work.id, &meta).await?;
    let content = versions::assemble_content(state, detail).await?;
    let chapters = content
        .chapters
        .into_iter()
        .map(|chapter| SourceChapter {
            id: chapter.chapter_id,
            title: chapter.title,
            role: Role::parse(&chapter.role),
            scenes: chapter
                .scenes
                .into_iter()
                .map(|scene| SourceScene {
                    id: scene.scene_id,
                    title: scene.title,
                    doc: scene.content,
                })
                .collect(),
        })
        .collect();
    Ok(Resolved {
        work,
        author,
        meta,
        cover,
        chapters,
        word_count: content.version.word_count,
        comments,
        use_positions: false,
        is_draft: false,
        version: Some(content.version),
    })
}

/// Stage 0, for the working draft: read it scene by scene.
pub(super) async fn resolve_draft(
    state: &AppState,
    user: AuthUser,
    work_id: String,
) -> ApiResult<Resolved> {
    let (work, author, outline, meta, comments) = blocking(&state.pool, move |conn| {
        let work = support::work_for_owner(conn, &work_id, &user)?;
        let author: User = users::table
            .find(&work.owner_id)
            .select(User::as_select())
            .first(conn)?;
        // Named as a version would name them, so an untitled chapter or scene
        // exports as "Chapter 2", not as nothing.
        let outline =
            super::works::with_placeholder_names(super::works::load_outline(conn, &work.id)?);
        let (meta, comments) = load_extras(conn, &work, &user)?;
        Ok((work, author, outline, meta, comments))
    })
    .await?;
    let cover = load_cover(state, &work.id, &meta).await?;

    let mut chapters = Vec::new();
    let mut words = 0;
    for chapter in outline.chapters {
        let mut scenes = Vec::new();
        for scene in chapter.scenes {
            words += scene.word_count;
            let doc = state
                .store
                .read_working_scene(&work.id, &scene.id)
                .await
                .unwrap_or_else(|_| crate::domain::doc::empty_doc());
            scenes.push(SourceScene {
                id: scene.id,
                title: scene.title,
                doc,
            });
        }
        chapters.push(SourceChapter {
            id: chapter.chapter.id,
            title: chapter.chapter.title,
            role: Role::parse(&chapter.chapter.role),
            scenes,
        });
    }
    Ok(Resolved {
        work,
        author,
        meta,
        cover,
        chapters,
        word_count: words,
        comments,
        use_positions: true,
        is_draft: true,
        version: None,
    })
}

/// A finished file and what was found while making it.
pub(super) struct BuiltFile {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub filename: String,
    pub issues: Vec<export::pipeline::Issue>,
}

/// Stages 1–4 for one request: refuse on errors, otherwise write the file.
pub(super) fn build_file(resolved: &Resolved, options: &ExportOptions) -> ApiResult<BuiltFile> {
    let report = assemble::preflight(resolved, options);
    if let Some(error) = report
        .issues
        .iter()
        .find(|i| i.severity == export::pipeline::Severity::Error)
    {
        return Err(ApiError::BadRequest(error.message.clone()));
    }
    let prepared = assemble::prepare(resolved, options);
    let Rendered { bytes, mime, .. } =
        assemble::render(&prepared.manuscript, options).map_err(|e| {
            ApiError::internal(format!(
                "export failed at the {} stage: {}",
                e.stage, e.message
            ))
        })?;
    Ok(BuiltFile {
        bytes,
        mime,
        filename: report.filename,
        issues: report.issues,
    })
}

pub(super) fn respond(file: BuiltFile) -> ApiResult<axum::response::Response> {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(file.mime));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", file.filename))
            .map_err(|e| ApiError::internal(e.to_string()))?,
    );
    let warnings = file
        .issues
        .iter()
        .filter(|i| i.severity == export::pipeline::Severity::Warning)
        .count();
    headers.insert(
        "x-export-warnings",
        HeaderValue::from_str(&warnings.to_string()).expect("digits are a valid header value"),
    );
    Ok((StatusCode::OK, headers, file.bytes).into_response())
}

/// Stages 1–4 for one request, as a download.
fn deliver(resolved: &Resolved, options: &ExportOptions) -> ApiResult<axum::response::Response> {
    respond(build_file(resolved, options)?)
}

/// Remembers what was sent: the options last used for this format, and a line
/// in the book's export history.
pub(super) async fn remember(
    state: &AppState,
    user: &AuthUser,
    resolved: &Resolved,
    options: &ExportOptions,
    file: &BuiltFile,
) -> ApiResult<()> {
    use crate::db::models::NewExportHistory;
    use crate::db::schema::{export_history, export_last_options};
    use sha2::{Digest, Sha256};

    let work_id = resolved.work.id.clone();
    let user_id = user.id.clone();
    let version_id = resolved.version.as_ref().map(|v| v.id.clone());
    let options_json = serde_json::to_string(options)?;
    let history = NewExportHistory {
        id: crate::domain::new_id(),
        work_id: work_id.clone(),
        user_id,
        version_id,
        format: options.format.extension().to_string(),
        profile_name: options.profile.clone(),
        options_hash: assemble::options_fingerprint(options),
        options: options_json.clone(),
        filename: file.filename.clone(),
        byte_size: file.bytes.len() as i32,
        content_hash: hex::encode(Sha256::digest(&file.bytes)),
        created_at: crate::domain::now_iso(),
    };
    let format = options.format.extension().to_string();
    blocking(&state.pool, move |conn| {
        let now = crate::domain::now_iso();
        diesel::replace_into(export_last_options::table)
            .values((
                export_last_options::work_id.eq(&work_id),
                export_last_options::format.eq(&format),
                export_last_options::options.eq(&options_json),
                export_last_options::used_at.eq(&now),
            ))
            .execute(conn)?;
        diesel::insert_into(export_history::table)
            .values(&history)
            .execute(conn)?;
        Ok(())
    })
    .await
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
) -> ApiResult<axum::response::Response> {
    let options = checked(shorthand(&query)?)?;
    let resolved = resolve_version(&state, user, version_id).await?;
    deliver(&resolved, &options)
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/export",
    tag = "versions",
    params(("version_id" = String, Path)),
    request_body = ExportOptions,
    responses(
        (status = 200, description = "The manuscript as a file", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export_version_with(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
    Json(options): Json<ExportOptions>,
) -> ApiResult<axum::response::Response> {
    let options = checked(options)?;
    let resolved = resolve_version(&state, user.clone(), version_id).await?;
    let file = build_file(&resolved, &options)?;
    remember(&state, &user, &resolved, &options, &file).await?;
    respond(file)
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/export",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The working draft as a file", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export_draft(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<axum::response::Response> {
    // The whole book, for another instance or for keeping.
    if query.format == "project" {
        return super::project::export_project(&state, user, work_id).await;
    }
    let options = checked(shorthand(&query)?)?;
    let resolved = resolve_draft(&state, user, work_id).await?;
    deliver(&resolved, &options)
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/export",
    tag = "works",
    params(("work_id" = String, Path)),
    request_body = ExportOptions,
    responses(
        (status = 200, description = "The working draft as a file", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export_draft_with(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(options): Json<ExportOptions>,
) -> ApiResult<axum::response::Response> {
    let options = checked(options)?;
    let resolved = resolve_draft(&state, user.clone(), work_id).await?;
    let file = build_file(&resolved, &options)?;
    remember(&state, &user, &resolved, &options, &file).await?;
    respond(file)
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/export/preflight",
    tag = "works",
    params(("work_id" = String, Path)),
    request_body = ExportOptions,
    responses(
        (status = 200, description = "What would go out, and what is wrong with it", body = PreflightReport),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn preflight_draft(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(options): Json<ExportOptions>,
) -> ApiResult<Json<PreflightReport>> {
    let options = checked(options)?;
    let resolved = resolve_draft(&state, user, work_id).await?;
    Ok(Json(assemble::preflight(&resolved, &options)))
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/export/preflight",
    tag = "versions",
    params(("version_id" = String, Path)),
    request_body = ExportOptions,
    responses(
        (status = 200, description = "What would go out, and what is wrong with it", body = PreflightReport),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn preflight_version(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
    Json(options): Json<ExportOptions>,
) -> ApiResult<Json<PreflightReport>> {
    let options = checked(options)?;
    let resolved = resolve_version(&state, user, version_id).await?;
    Ok(Json(assemble::preflight(&resolved, &options)))
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
            id: chapter.chapter_id,
            title: chapter.title,
            role: Role::parse(&chapter.role),
            notes: Vec::new(),
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
                        id: scene.scene_id,
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
        language: "en".into(),
        ..Default::default()
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
    let options = checked(shorthand_for(&query)?)?;

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

    if !matches!(options.format, Format::Docx | Format::Pdf | Format::Txt) {
        return Err(ApiError::BadRequest(format!(
            "format must be one of docx, pdf, txt — got `{}`",
            query.format
        )));
    }
    let Rendered {
        bytes,
        mime,
        extension,
    } = assemble::render(&manuscript, &options).map_err(|e| {
        ApiError::internal(format!(
            "export failed at the {} stage: {}",
            e.stage, e.message
        ))
    })?;

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

    /// Comments are private to whoever wrote them. A co-author's comments sit
    /// on the same draft, and must never travel in the owner's file.
    #[test]
    fn only_the_exporters_own_comments_are_loaded() {
        let root = std::env::temp_dir().join(format!("justtype-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let pool = crate::db::build_pool(root.join("t.db").to_str().unwrap()).unwrap();
        crate::db::run_migrations(&pool).unwrap();
        let mut conn = pool.get().unwrap();
        let now = crate::domain::now_iso();
        let run = |conn: &mut crate::db::Conn, sql: String| {
            diesel::sql_query(sql).execute(conn).unwrap();
        };
        for (id, email) in [("u1", "a@example.com"), ("u2", "b@example.com")] {
            run(&mut conn, format!(
                "INSERT INTO users (id, email, display_name, password_hash, created_at, updated_at) \
                 VALUES ('{id}', '{email}', '{id}', '!', '{now}', '{now}');"));
        }
        run(&mut conn, format!(
            "INSERT INTO works (id, owner_id, title, kind, status, word_count, created_at, updated_at) \
             VALUES ('w1', 'u1', 'T', 'novel', 'draft', 0, '{now}', '{now}');"));
        run(
            &mut conn,
            format!(
                "INSERT INTO chapters (id, work_id, title, position, created_at, updated_at) \
             VALUES ('c1', 'w1', 'One', 0, '{now}', '{now}');"
            ),
        );
        run(&mut conn, format!(
            "INSERT INTO scenes (id, work_id, chapter_id, title, status, position, word_count, content_hash, created_at, updated_at) \
             VALUES ('s1', 'w1', 'c1', 'S', 'drafting', 0, 0, 'h', '{now}', '{now}');"));
        for (id, user, kind, note) in [
            ("a1", "u1", "comment", "mine"),
            ("a2", "u2", "comment", "theirs"),
            ("a3", "u1", "highlight", "no note kind"),
        ] {
            run(&mut conn, format!(
                "INSERT INTO annotations (id, user_id, work_id, scene_id, kind, from_pos, to_pos, quote, note, created_at, updated_at) \
                 VALUES ('{id}', '{user}', 'w1', 's1', '{kind}', 1, 4, 'abc', '{note}', '{now}', '{now}');"));
        }

        let work: Work = crate::db::schema::works::table
            .find("w1")
            .select(Work::as_select())
            .first(&mut conn)
            .unwrap();
        let owner = AuthUser {
            id: "u1".into(),
            email: "a@example.com".into(),
            email_verified: true,
            session_id: "x".into(),
        };
        let (_, mine) = load_extras(&mut conn, &work, &owner).unwrap();
        assert_eq!(
            mine.len(),
            1,
            "only the exporter's own comments, and only comments"
        );
        assert_eq!(mine[0].note, "mine");

        let other = AuthUser {
            id: "u2".into(),
            ..owner
        };
        let (_, theirs) = load_extras(&mut conn, &work, &other).unwrap();
        assert_eq!(theirs.len(), 1);
        assert_eq!(theirs[0].note, "theirs");
        std::fs::remove_dir_all(&root).ok();
    }
}
