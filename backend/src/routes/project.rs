//! A whole book out as a project zip, and projects and single documents in.
//!
//! The zip's format is in `export::project`. Importing always makes new rows
//! with new ids — a project carries its own ids only so its notebook links can
//! be followed — so the same zip imported twice makes two books, and importing
//! into an instance that already has the original never collides with it.
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use diesel::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{chapters, notebook_entries, scene_notebook_links, scenes, works};
use crate::domain::enums::{ActivityKind, Genre, NotebookKind, SceneStatus, WorkKind};
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::export::markdown;
use crate::export::project::{
    self, Project, ProjectChapter, ProjectEntry, ProjectScene, ProjectWork, FORMAT, FORMAT_VERSION,
};
use crate::state::AppState;

use super::collaborators::record;
use super::export::safe_filename;
use super::support::{self, Ability};
use super::works::{load_outline, outline_for, OutlineResponse};

/// The most one import will create, whatever the zip claims.
const MAX_CHAPTERS: usize = 2_000;
const MAX_SCENES: usize = 10_000;
const MAX_ENTRIES: usize = 10_000;
/// The largest file an import will take, in bytes.
pub const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;

// ── Export ──────────────────────────────────────────────────────────────────

/// The working draft as a project zip.
pub async fn export_project(
    state: &AppState,
    user: AuthUser,
    work_id: String,
) -> ApiResult<Response> {
    let (work, outline, entries, links, meta) = blocking(&state.pool, move |conn| {
        let work = support::work_for_owner(conn, &work_id, &user)?;
        let meta = crate::db::schema::work_metadata::table
            .find(&work.id)
            .select(crate::db::models::WorkMetadata::as_select())
            .first(conn)
            .optional()?;
        let outline = load_outline(conn, &work.id)?;
        let entries: Vec<NotebookEntry> = notebook_entries::table
            .filter(notebook_entries::work_id.eq(&work.id))
            .order(notebook_entries::created_at.asc())
            .select(NotebookEntry::as_select())
            .load(conn)?;
        let scene_ids: Vec<String> = outline
            .chapters
            .iter()
            .flat_map(|c| c.scenes.iter().map(|s| s.id.clone()))
            .collect();
        let links: Vec<SceneNotebookLink> = scene_notebook_links::table
            .filter(scene_notebook_links::scene_id.eq_any(&scene_ids))
            .select(SceneNotebookLink::as_select())
            .load(conn)?;
        Ok((work, outline, entries, links, meta))
    })
    .await?;

    let cover = match meta.as_ref().and_then(|m| m.cover_mime.clone()) {
        Some(mime) => state
            .store
            .read_cover(&work.id)
            .await?
            .map(|bytes| project::ProjectCover { mime, bytes }),
        None => None,
    };

    let mut project_chapters = Vec::new();
    for chapter in outline.chapters {
        let mut project_scenes = Vec::new();
        for scene in chapter.scenes {
            let content = state
                .store
                .read_working_scene(&work.id, &scene.id)
                .await
                .unwrap_or_else(|_| doc::empty_doc());
            project_scenes.push(ProjectScene {
                notebook: links
                    .iter()
                    .filter(|l| l.scene_id == scene.id)
                    .map(|l| l.entry_id.clone())
                    .collect(),
                id: scene.id,
                title: scene.title,
                summary: scene.summary,
                pov: scene.pov,
                status: Some(scene.status),
                target_words: scene.target_words,
                location: scene.location,
                story_date: scene.story_date,
                file: None,
                content,
            });
        }
        project_chapters.push(ProjectChapter {
            id: chapter.chapter.id,
            title: chapter.chapter.title,
            summary: chapter.chapter.summary,
            role: Some(chapter.chapter.role),
            scenes: project_scenes,
        });
    }

    let title = work.title.clone();
    let mut project = Project {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        exported_at: now_iso(),
        work: ProjectWork {
            title: work.title,
            subtitle: work.subtitle,
            kind: Some(work.kind),
            synopsis: work.synopsis,
            pen_name: work.pen_name,
            short_title: work.short_title,
            dedication: work.dedication,
            epigraph: work.epigraph,
            epigraph_source: work.epigraph_source,
            copyright_line: work.copyright_line,
            genre: work.genre,
            target_words: work.target_words,
            target_date: work.target_date,
            metadata: meta.map(|m| project::ProjectMetadata {
                language: m.language,
                text_direction: m.text_direction,
                description: m.description,
                publisher: m.publisher,
                published: m.published,
                identifier: m.identifier,
                keywords: m
                    .keywords
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_default(),
                contributors: m
                    .contributors
                    .as_deref()
                    .and_then(|s| {
                        serde_json::from_str::<Vec<super::book_details::ContributorBody>>(s).ok()
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| project::ProjectContributor {
                        name: c.name,
                        role: c.role,
                    })
                    .collect(),
                cover_file: None,
            }),
        },
        cover,
        chapters: project_chapters,
        notebook: entries
            .into_iter()
            .map(|e| ProjectEntry {
                id: e.id,
                kind: e.kind,
                name: e.name,
                summary: e.summary,
                body: e.body,
                fields: e.fields.and_then(|raw| serde_json::from_str(&raw).ok()),
            })
            .collect(),
    };

    let bytes = tokio::task::spawn_blocking(move || project::write(&mut project))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            safe_filename(&title, "zip")
        ))
        .map_err(|e| ApiError::internal(e.to_string()))?,
    );
    Ok((StatusCode::OK, headers, bytes).into_response())
}

// ── Import ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ImportQuery {
    /// The file's name, which says what it is and, for a document, what to call
    /// it.
    pub name: String,
    /// For a document: the chapter to put it in. Default the last one.
    #[serde(default)]
    pub chapter_id: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Project,
    Document,
}

fn kind_of(name: &str, body: &[u8]) -> ApiResult<FileKind> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".zip") || body.starts_with(b"PK") {
        Ok(FileKind::Project)
    } else if lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".txt") {
        Ok(FileKind::Document)
    } else {
        Err(ApiError::BadRequest(
            "import a project (.zip) or a document (.md, .markdown or .txt)".into(),
        ))
    }
}

/// "The Long Way Home.md" → "The Long Way Home".
fn stem(name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let base = leaf.rsplit_once('.').map(|(s, _)| s).unwrap_or(leaf);
    project::clean_name(base)
}

fn document_from(body: &[u8]) -> ApiResult<Value> {
    let text = std::str::from_utf8(body)
        .map_err(|_| ApiError::BadRequest("that document is not text".into()))?;
    Ok(markdown::parse(text.trim_start_matches('\u{feff}')))
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// A zip becomes a book, a document becomes a book of one scene.
#[utoipa::path(
    post,
    path = "/api/works/import",
    tag = "works",
    params(ImportQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "The new book", body = OutlineResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn import_work(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ImportQuery>,
    body: Bytes,
) -> ApiResult<Json<OutlineResponse>> {
    if body.is_empty() {
        return Err(ApiError::BadRequest("that file is empty".into()));
    }
    let file_stem = stem(&query.name);
    let mut project = match kind_of(&query.name, &body)? {
        FileKind::Project => {
            let bytes = body.to_vec();
            tokio::task::spawn_blocking(move || project::read(&bytes))
                .await
                .map_err(|e| ApiError::internal(e.to_string()))?
                .map_err(ApiError::BadRequest)?
        }
        FileKind::Document => Project {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            work: ProjectWork::default(),
            chapters: vec![ProjectChapter {
                scenes: vec![ProjectScene {
                    title: file_stem.clone(),
                    status: Some("drafting".into()),
                    content: document_from(&body)?,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    // A project without a title of its own is named for its file.
    if project.work.title.trim().is_empty() || project.work.title == project::FALLBACK_TITLE {
        project.work.title = if file_stem.is_empty() {
            "Imported book".into()
        } else {
            file_stem
        };
    }

    let outline = create_from(&state, user, project).await?;
    Ok(Json(outline))
}

/// Makes the book a project describes, for `user`.
async fn create_from(
    state: &AppState,
    user: AuthUser,
    project: Project,
) -> ApiResult<OutlineResponse> {
    let title = project.work.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::BadRequest("that project has no title".into()));
    }
    let scene_count: usize = project.chapters.iter().map(|c| c.scenes.len()).sum();
    if project.chapters.len() > MAX_CHAPTERS
        || scene_count > MAX_SCENES
        || project.notebook.len() > MAX_ENTRIES
    {
        return Err(ApiError::BadRequest(
            "that project is larger than an import takes".into(),
        ));
    }
    for chapter in &project.chapters {
        for scene in &chapter.scenes {
            doc::validate(&scene.content).map_err(|e| {
                ApiError::BadRequest(format!("a scene in that project has no valid text: {e}"))
            })?;
        }
    }

    let work_id = new_id();
    let db_work_id = work_id.clone();
    let creator = user.clone();
    let cover_bytes = project.cover.as_ref().map(|c| c.bytes.clone());
    let (outline, pending, pending_cover) = blocking(&state.pool, move |conn| {
        let now = now_iso();
        let w = &project.work;
        let mut pending: Vec<(String, Value)> = Vec::new();
        let mut pending_cover = false;

        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::insert_into(works::table)
                .values(&NewWork {
                    id: db_work_id.clone(),
                    owner_id: creator.id.clone(),
                    title: title.clone(),
                    subtitle: clean(w.subtitle.clone()),
                    kind: w
                        .kind
                        .clone()
                        .filter(|k| WorkKind::accepts(k))
                        .unwrap_or_else(|| "novel".into()),
                    synopsis: clean(w.synopsis.clone()),
                    template_id: None,
                    // A book arriving from elsewhere starts as a draft, whatever
                    // it was there: its versions and readers did not come along.
                    status: "draft".into(),
                    current_version_id: None,
                    word_count: 0,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    pen_name: clean(w.pen_name.clone()),
                    short_title: clean(w.short_title.clone()),
                    dedication: clean(w.dedication.clone()),
                    epigraph: clean(w.epigraph.clone()),
                    epigraph_source: clean(w.epigraph_source.clone()),
                    copyright_line: clean(w.copyright_line.clone()),
                    pinned_version_id: None,
                    genre: w.genre.clone().filter(|g| Genre::accepts(g)),
                    target_words: w.target_words.filter(|n| *n > 0),
                    target_date: clean(w.target_date.clone()),
                })
                .execute(conn)?;

            // The notebook first, so scenes can link to it.
            let mut entry_ids: HashMap<String, String> = HashMap::new();
            for entry in &project.notebook {
                if !NotebookKind::accepts(&entry.kind) || entry.name.trim().is_empty() {
                    continue;
                }
                let id = new_id();
                diesel::insert_into(notebook_entries::table)
                    .values(&NewNotebookEntry {
                        id: id.clone(),
                        work_id: db_work_id.clone(),
                        kind: entry.kind.clone(),
                        name: entry.name.trim().to_string(),
                        summary: clean(entry.summary.clone()),
                        body: clean(entry.body.clone()),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                        fields: entry
                            .fields
                            .as_ref()
                            .filter(|f| !f.is_null())
                            .map(|f| f.to_string()),
                        template_id: None,
                    })
                    .execute(conn)?;
                entry_ids.insert(entry.id.clone(), id);
            }

            for (c, chapter) in project.chapters.iter().enumerate() {
                let chapter_id = new_id();
                diesel::insert_into(chapters::table)
                    .values(&NewChapter {
                        id: chapter_id.clone(),
                        work_id: db_work_id.clone(),
                        title: chapter.title.trim().to_string(),
                        summary: clean(chapter.summary.clone()),
                        position: c as i32,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                        role: chapter
                            .role
                            .clone()
                            .filter(|r| matches!(r.as_str(), "chapter" | "front" | "back"))
                            .unwrap_or_else(|| "chapter".into()),
                    })
                    .execute(conn)?;

                for (s, scene) in chapter.scenes.iter().enumerate() {
                    let scene_id = new_id();
                    diesel::insert_into(scenes::table)
                        .values(&NewScene {
                            id: scene_id.clone(),
                            work_id: db_work_id.clone(),
                            chapter_id: chapter_id.clone(),
                            title: scene.title.trim().to_string(),
                            summary: clean(scene.summary.clone()),
                            pov: clean(scene.pov.clone()),
                            status: scene
                                .status
                                .clone()
                                .filter(|s| SceneStatus::accepts(s))
                                .unwrap_or_else(|| "drafting".into()),
                            position: s as i32,
                            word_count: doc::word_count(&scene.content),
                            content_hash: doc::content_hash(&scene.content),
                            created_at: now.clone(),
                            updated_at: now.clone(),
                            target_words: scene.target_words.filter(|n| *n > 0),
                            location: clean(scene.location.clone()),
                            story_date: clean(scene.story_date.clone()),
                        })
                        .execute(conn)?;
                    for old in &scene.notebook {
                        if let Some(entry_id) = entry_ids.get(old) {
                            diesel::insert_into(scene_notebook_links::table)
                                .values(&NewSceneNotebookLink {
                                    id: new_id(),
                                    scene_id: scene_id.clone(),
                                    entry_id: entry_id.clone(),
                                    created_at: now.clone(),
                                })
                                .execute(conn)?;
                        }
                    }
                    pending.push((scene_id, scene.content.clone()));
                }
            }

            if let Some(meta) = &w.metadata {
                use crate::db::schema::work_metadata;
                let language =
                    clean(meta.language.clone()).filter(|l| super::book_details::valid_language(l));
                diesel::insert_into(work_metadata::table)
                    .values((
                        work_metadata::work_id.eq(&db_work_id),
                        work_metadata::language.eq(language),
                        work_metadata::text_direction
                            .eq(clean(meta.text_direction.clone())
                                .filter(|d| d == "ltr" || d == "rtl")),
                        work_metadata::description.eq(clean(meta.description.clone())),
                        work_metadata::publisher.eq(clean(meta.publisher.clone())),
                        work_metadata::published.eq(clean(meta.published.clone())),
                        work_metadata::identifier.eq(clean(meta.identifier.clone())),
                        work_metadata::keywords.eq((!meta.keywords.is_empty()).then(|| {
                            serde_json::to_string(&meta.keywords).expect("strings serialise")
                        })),
                        work_metadata::contributors.eq((!meta.contributors.is_empty()).then(
                            || {
                                serde_json::to_string(&meta.contributors)
                                    .expect("contributors serialise")
                            },
                        )),
                        work_metadata::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
                if let Some(cover) = &project.cover {
                    if let Some((mime, w, h)) = super::book_details::sniff_image(&cover.bytes) {
                        diesel::update(work_metadata::table.find(&db_work_id))
                            .set((
                                work_metadata::cover_mime.eq(Some(mime)),
                                work_metadata::cover_bytes.eq(Some(cover.bytes.len() as i32)),
                                work_metadata::cover_width.eq(Some(w as i32)),
                                work_metadata::cover_height.eq(Some(h as i32)),
                            ))
                            .execute(conn)?;
                        pending_cover = true;
                    }
                }
            }

            support::recount_work(conn, &db_work_id, &now)?;
            Ok(())
        })?;

        let outline = outline_for(conn, &db_work_id, &creator)?;
        Ok((outline, pending, pending_cover))
    })
    .await?;

    for (scene_id, content) in pending {
        state
            .store
            .write_working_scene(&work_id, &scene_id, &content)
            .await?;
    }
    if pending_cover {
        if let Some(cover) = &cover_bytes {
            state.store.write_cover(&work_id, cover).await?;
        }
    }
    Ok(outline)
}

/// A document becomes a new scene in a book that already exists.
#[utoipa::path(
    post,
    path = "/api/works/{work_id}/import",
    tag = "works",
    params(("work_id" = String, Path), ImportQuery),
    request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "The new scene", body = Scene),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn import_document(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<ImportQuery>,
    body: Bytes,
) -> ApiResult<Json<Scene>> {
    if body.is_empty() {
        return Err(ApiError::BadRequest("that file is empty".into()));
    }
    if kind_of(&query.name, &body)? == FileKind::Project {
        return Err(ApiError::BadRequest(
            "that is a whole project: import it from the library, where it becomes a book of its own"
                .into(),
        ));
    }
    let content = document_from(&body)?;
    doc::validate(&content).map_err(ApiError::BadRequest)?;
    let title = stem(&query.name);
    let content_for_db = content.clone();

    let scene = blocking(&state.pool, move |conn| {
        let chapter_id = match &query.chapter_id {
            Some(id) => {
                let (chapter, _) = support::chapter_for(conn, id, &user, Ability::Write)?;
                if chapter.work_id != work_id {
                    return Err(ApiError::BadRequest(
                        "that chapter is not in this book".into(),
                    ));
                }
                chapter.id
            }
            None => {
                support::whole_work_for(conn, &work_id, &user, Ability::Write)?;
                let last: Option<String> = chapters::table
                    .filter(chapters::work_id.eq(&work_id))
                    .order(chapters::position.desc())
                    .select(chapters::id)
                    .first(conn)
                    .optional()?;
                match last {
                    Some(id) => id,
                    None => {
                        let now = now_iso();
                        let id = new_id();
                        diesel::insert_into(chapters::table)
                            .values(&NewChapter {
                                id: id.clone(),
                                work_id: work_id.clone(),
                                title: String::new(),
                                summary: None,
                                position: 0,
                                created_at: now.clone(),
                                updated_at: now,
                                role: "chapter".into(),
                            })
                            .execute(conn)?;
                        id
                    }
                }
            }
        };
        let now = now_iso();
        let row = NewScene {
            id: new_id(),
            work_id: work_id.clone(),
            chapter_id: chapter_id.clone(),
            title,
            summary: None,
            pov: None,
            status: "drafting".into(),
            position: support::next_position(conn, &chapter_id)?,
            word_count: doc::word_count(&content_for_db),
            content_hash: doc::content_hash(&content_for_db),
            created_at: now.clone(),
            updated_at: now.clone(),
            target_words: None,
            location: None,
            story_date: None,
        };
        diesel::insert_into(scenes::table)
            .values(&row)
            .execute(conn)?;
        support::recount_work(conn, &work_id, &now)?;
        record(
            conn,
            &work_id,
            &user.id,
            ActivityKind::SceneAdded,
            Some(&row.id),
            &row.title,
        )?;
        Ok(scenes::table
            .find(&row.id)
            .select(Scene::as_select())
            .first(conn)?)
    })
    .await?;

    state
        .store
        .write_working_scene(&scene.work_id, &scene.id, &content)
        .await?;
    Ok(Json(scene))
}
