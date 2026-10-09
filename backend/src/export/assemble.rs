//! From the book as stored to a file: assemble the manuscript the options
//! describe, say what is wrong with it, and write it out.
//!
//! Nothing here touches the database or the disk; the route loads a
//! [`Resolved`] and this does the rest, which is why preflight can reuse every
//! stage up to rendering and why each stage can be tested on its own.
use serde::Serialize;

use crate::db::models::{User, Version, Work, WorkMetadata};

use super::options::{ExportOptions, Format, HtmlMode, StylePreset};
use super::pipeline::{self, Issue, Severity, SourceChapter, SourceComment, Summary};
use super::{Contact, Contributor, Cover, Manuscript, Role, SceneContent, Span, Style};

/// Everything loaded for an export, before any option is applied.
pub struct Resolved {
    pub work: Work,
    pub author: User,
    pub meta: Option<WorkMetadata>,
    pub cover: Option<Cover>,
    pub chapters: Vec<SourceChapter>,
    pub word_count: i32,
    pub comments: Vec<SourceComment>,
    /// A working draft's comment positions mean something; a version's do not.
    pub use_positions: bool,
    pub is_draft: bool,
    /// The snapshot being exported, when it is one.
    pub version: Option<Version>,
}

pub struct Prepared {
    pub manuscript: Manuscript,
    pub issues: Vec<Issue>,
    pub summary: Summary,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub summary: Summary,
    pub issues: Vec<Issue>,
    /// False when any issue is an error. Warnings never block.
    pub ok: bool,
    /// The name the download will have, so the dialog can show it as the
    /// template is typed.
    pub filename: String,
}

pub struct Rendered {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub extension: &'static str,
}

/// A writer failure, naming the stage and what to do about it.
#[derive(Debug)]
pub struct RenderError {
    pub stage: &'static str,
    pub message: String,
}

fn parse_list<T: serde::de::DeserializeOwned>(raw: &Option<String>) -> Vec<T> {
    raw.as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

#[derive(serde::Deserialize)]
struct ContributorRow {
    name: String,
    role: String,
}

/// Languages written right to left.
pub fn is_rtl(language: &str) -> bool {
    let primary = language.split(['-', '_']).next().unwrap_or("");
    matches!(
        primary.to_ascii_lowercase().as_str(),
        "ar" | "he" | "fa" | "ur" | "yi" | "ps" | "sd" | "dv" | "ug"
    )
}

fn clean(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Stages 1–3: select, build, and put the work's own details on top.
pub fn prepare(resolved: &Resolved, options: &ExportOptions) -> Prepared {
    let mut chapters = pipeline::select(resolved.chapters.clone(), options);

    // About the author is the writer's own, reusable across books, and goes in
    // as back matter — so it only goes in where back matter does.
    let about = clean(&resolved.author.about_author);
    if options.content.about_author && options.include_back_matter() {
        if let Some(text) = &about {
            chapters.push(SourceChapter {
                id: String::new(),
                title: "About the author".into(),
                role: Role::Back,
                scenes: vec![pipeline::SourceScene {
                    id: String::new(),
                    title: String::new(),
                    doc: serde_json::json!({"type": "doc", "content": text
                        .split("\n\n")
                        .filter(|p| !p.trim().is_empty())
                        .map(|p| serde_json::json!({
                            "type": "paragraph",
                            "content": [{"type": "text", "text": p.trim()}]
                        }))
                        .collect::<Vec<_>>()}),
                }],
            });
        }
    }

    let built = pipeline::build(
        chapters,
        &resolved.comments,
        options,
        resolved.use_positions,
    );

    let work = &resolved.work;
    let overrides = &options.metadata;
    let meta = resolved.meta.as_ref();

    let byline = clean(&overrides.author)
        .or_else(|| work.pen_name.clone())
        .unwrap_or_else(|| resolved.author.display_name.clone());
    // The header wants a surname. The last whitespace-separated word is a guess,
    // and a wrong one for plenty of names — but it is the writer's own byline,
    // and they can override it by setting a short title.
    let surname = byline
        .split_whitespace()
        .last()
        .unwrap_or(&byline)
        .to_string();

    let title = clean(&overrides.title).unwrap_or_else(|| work.title.clone());
    let language = clean(&overrides.language)
        .or_else(|| meta.and_then(|m| clean(&m.language)))
        .unwrap_or_else(|| "en".into());
    let rtl = match meta.and_then(|m| m.text_direction.as_deref()) {
        Some("rtl") => true,
        Some("ltr") => false,
        _ => is_rtl(&language),
    };

    let manuscript = Manuscript {
        short_title: if overrides.title.is_some() {
            title.clone()
        } else {
            work.short_title.clone().unwrap_or_else(|| title.clone())
        },
        title,
        subtitle: clean(&overrides.subtitle).or_else(|| work.subtitle.clone()),
        byline,
        surname,
        contact: Contact {
            legal_name: resolved.author.legal_name.clone(),
            address: resolved.author.mailing_address.clone(),
            phone: resolved.author.phone.clone(),
            email: Some(resolved.author.email.clone()),
        },
        copyright_line: work.copyright_line.clone(),
        dedication: work.dedication.clone(),
        epigraph: work.epigraph.clone(),
        epigraph_source: work.epigraph_source.clone(),
        word_count: resolved.word_count,
        chapters: built.chapters,
        language,
        rtl,
        description: clean(&overrides.description)
            .or_else(|| meta.and_then(|m| clean(&m.description))),
        publisher: clean(&overrides.publisher).or_else(|| meta.and_then(|m| clean(&m.publisher))),
        published: clean(&overrides.published).or_else(|| meta.and_then(|m| clean(&m.published))),
        identifier: clean(&overrides.identifier)
            .or_else(|| meta.and_then(|m| clean(&m.identifier))),
        keywords: meta.map(|m| parse_list(&m.keywords)).unwrap_or_default(),
        contributors: meta
            .map(|m| parse_list::<ContributorRow>(&m.contributors))
            .unwrap_or_default()
            .into_iter()
            .map(|c| Contributor {
                name: c.name,
                role: c.role,
            })
            .collect(),
        cover: resolved.cover.clone(),
        omit_front_matter: !options.content.front_matter,
        toc: options.include_toc(),
        note_layout: built.note_layout,
        note_label: options.content.comment_label,
        frozen_at: resolved
            .version
            .as_ref()
            .map(|v| format!("{}Z", v.created_at.chars().take(19).collect::<String>())),
        frozen_id: resolved.version.as_ref().map(|v| format!("{}:{}", v.id, options_fingerprint(options))),
    };

    Prepared {
        manuscript,
        issues: built.issues,
        summary: built.summary,
    }
}

/// The checks that need the whole picture. Errors block the download;
/// warnings never do.
pub fn preflight(resolved: &Resolved, options: &ExportOptions) -> PreflightReport {
    let prepared = prepare(resolved, options);
    let mut issues = prepared.issues;
    let manuscript = &prepared.manuscript;

    if manuscript.is_empty() {
        let selecting = matches!(
            options.content.scope,
            super::options::Scope::Selected { .. }
        );
        issues.insert(
            0,
            Issue {
                severity: Severity::Error,
                code: if selecting {
                    "nothing_selected"
                } else {
                    "nothing_to_export"
                }
                .into(),
                message: if selecting {
                    "Nothing is selected. Tick at least one chapter or scene that has text in it."
                        .into()
                } else if resolved.is_draft {
                    "There is nothing to export: nothing has been written yet. Write a scene, \
                     then export again."
                        .into()
                } else {
                    "There is nothing to export: this version has no text in it. Export the \
                     working draft instead."
                        .into()
                },
                location: None,
            },
        );
    }

    // Standard manuscript format expects to start at the beginning.
    if options.style.preset == StylePreset::Manuscript
        && matches!(options.format, Format::Docx | Format::Pdf)
    {
        if let Some(first) = resolved
            .chapters
            .iter()
            .find(|c| c.role == Role::Chapter && c.scenes.iter().any(|s| s.has_text()))
        {
            let included = prepared
                .manuscript
                .chapters
                .iter()
                .any(|c| c.id == first.id);
            if !included && !manuscript.is_empty() {
                issues.push(Issue {
                    severity: Severity::Warning,
                    code: "first_chapter_excluded".into(),
                    message: format!(
                        "The selection leaves out “{}”, which standard manuscript format \
                         expects to open with. Tick it, or choose the book layout.",
                        first.title
                    ),
                    location: Some(pipeline::Location {
                        chapter_id: first.id.clone(),
                        scene_id: None,
                    }),
                });
            }
        }
    }

    let wants_cover = options.format == Format::Epub
        || (options.format == Format::Html && options.html().mode == HtmlMode::Package);
    if wants_cover && manuscript.cover.is_none() && !manuscript.is_empty() {
        issues.push(Issue {
            severity: Severity::Warning,
            code: "no_cover".into(),
            message: "There is no cover image, so e-readers will show a blank tile. Add one \
                      under Book → Details."
                .into(),
            location: None,
        });
    }

    if manuscript.language == "en"
        && resolved
            .meta
            .as_ref()
            .and_then(|m| m.language.as_ref())
            .is_none()
        && options.metadata.language.is_none()
        && matches!(options.format, Format::Epub | Format::Html)
    {
        issues.push(Issue {
            severity: Severity::Info,
            code: "language_assumed".into(),
            message: "No language is set, so the file says English. Set it under Book → Details \
                      if the book is in another language."
                .into(),
            location: None,
        });
    }

    if options.style.preset == StylePreset::Manuscript && options.style != Default::default() {
        let customised = options.style.resolve();
        let _ = customised;
        if options.style.font.is_some()
            || options.style.size_pt.is_some()
            || options.style.scene_break.is_some()
            || options.style.line_spacing.is_some()
        {
            issues.push(Issue {
                severity: Severity::Info,
                code: "style_fixed".into(),
                message: "Standard manuscript format fixes the typography; your font, size and \
                          spacing choices are kept for the book layout."
                    .into(),
                location: None,
            });
        }
    }

    let ok = !issues.iter().any(|i| i.severity == Severity::Error);
    let filename = filename(options, resolved, &prepared.manuscript);
    PreflightReport {
        summary: prepared.summary,
        issues,
        ok,
        filename,
    }
}

/// Stage 4: write the file.
pub fn render(manuscript: &Manuscript, options: &ExportOptions) -> Result<Rendered, RenderError> {
    let failed = |format: &str, e: std::io::Error| RenderError {
        stage: "render",
        message: format!(
            "The {format} file could not be written ({e}). Try again; if it keeps failing, \
             export another format and report this."
        ),
    };
    let style: Style = options.style_kind();
    let rendered = match options.format {
        Format::Docx => Rendered {
            bytes: super::docx::write(manuscript, options).map_err(|e| failed("DOCX", e))?,
            mime: Format::Docx.mime(),
            extension: "docx",
        },
        Format::Epub => Rendered {
            bytes: super::epub::write(manuscript, options).map_err(|e| failed("EPUB", e))?,
            mime: Format::Epub.mime(),
            extension: "epub",
        },
        Format::Pdf => Rendered {
            bytes: super::pdf::write(manuscript, options),
            mime: Format::Pdf.mime(),
            extension: "pdf",
        },
        Format::Html => {
            if options.html().mode == HtmlMode::Package {
                Rendered {
                    bytes: super::html::write_package(manuscript, options)
                        .map_err(|e| failed("HTML package", e))?,
                    mime: "application/zip",
                    extension: "zip",
                }
            } else {
                Rendered {
                    bytes: super::html::write(manuscript, options).into_bytes(),
                    mime: Format::Html.mime(),
                    extension: "html",
                }
            }
        }
        Format::Txt => Rendered {
            bytes: super::text::write(manuscript, options).into_bytes(),
            mime: Format::Txt.mime(),
            extension: "txt",
        },
    };
    let _ = style;
    Ok(rendered)
}

impl pipeline::SourceScene {
    /// Whether the document has anything a reader would see.
    pub fn has_text(&self) -> bool {
        !super::blocks_from_doc(&self.doc).is_empty()
    }
}

/// Used by tests and by the empty-selection message.
pub fn scene_words(scene: &SceneContent) -> usize {
    scene
        .blocks
        .iter()
        .map(|b| match b {
            super::Block::Paragraph(spans) | super::Block::Quote(spans) => spans
                .iter()
                .map(|s: &Span| s.text.split_whitespace().count())
                .sum(),
            super::Block::Heading { spans, .. } => spans
                .iter()
                .map(|s: &Span| s.text.split_whitespace().count())
                .sum(),
            super::Block::SceneBreak => 0,
        })
        .sum()
}

/// A short, stable hash of the options that shape the file, for deriving
/// repeatable ids and for the export history.
pub fn options_fingerprint(options: &ExportOptions) -> String {
    use sha2::{Digest, Sha256};
    let mut canonical = options.clone();
    canonical.profile = None;
    let json = serde_json::to_string(&canonical).unwrap_or_default();
    hex::encode(&Sha256::digest(json.as_bytes())[..8])
}

/// Everything after the last path separator, minus anything a filesystem or a
/// `Content-Disposition` header would rather not see.
pub fn safe_filename(title: &str, extension: &str) -> String {
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

/// The file extension a set of options produces: a packaged HTML site is a zip.
pub fn extension_of(options: &ExportOptions) -> &'static str {
    match (options.format, options.html().mode) {
        (Format::Html, HtmlMode::Package) => "zip",
        (format, _) => format.extension(),
    }
}

/// The download's name: the filename template filled in, then made safe.
pub fn filename(options: &ExportOptions, resolved: &Resolved, manuscript: &Manuscript) -> String {
    let template = options.filename_template.as_deref().unwrap_or("{title}");
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let version = resolved
        .version
        .as_ref()
        .map(|v| v.label.clone())
        .unwrap_or_else(|| "draft".into());
    let stem = super::options::fill_template(
        template,
        &[
            ("title", &manuscript.title),
            ("short_title", &manuscript.short_title),
            ("author", &manuscript.byline),
            ("date", &date),
            ("version", &version),
            ("format", options.format.extension()),
        ],
    );
    safe_filename(&stem, extension_of(options))
}
