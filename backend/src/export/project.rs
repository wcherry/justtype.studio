//! A whole book as one zip, for backup, for moving between instances of Just
//! Type Studio, and for working on in a text editor.
//!
//! ```text
//! project.json                      everything needed to rebuild the book
//! README.txt                        what the files are
//! manuscript/01-chapter-one/01-the-door.md    one Markdown file per scene
//! ```
//!
//! `project.json` is the source of truth: it carries the book's details, the
//! structure, every scene's exact document and settings, and the notebook, so
//! importing it reproduces the book. The Markdown files are the same scenes in
//! a form any editor opens; a zip with only those (no `project.json`) imports
//! too, one chapter per folder and one scene per file.
use std::io::{Cursor, Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::markdown;

pub const FORMAT: &str = "justtype-project";
pub const FORMAT_VERSION: u32 = 1;

/// The title a project gets when its zip has no `project.json` to name it; the
/// importer swaps it for the file's name.
pub const FALLBACK_TITLE: &str = "Imported project";

/// Most entries and bytes an import will read, so a hostile or mistaken zip
/// cannot fill the server's memory.
const MAX_ENTRIES: usize = 20_000;
const MAX_ENTRY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub format: String,
    pub format_version: u32,
    #[serde(default)]
    pub exported_at: String,
    pub work: ProjectWork,
    #[serde(default)]
    pub chapters: Vec<ProjectChapter>,
    #[serde(default)]
    pub notebook: Vec<ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWork {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub synopsis: Option<String>,
    #[serde(default)]
    pub pen_name: Option<String>,
    #[serde(default)]
    pub short_title: Option<String>,
    #[serde(default)]
    pub dedication: Option<String>,
    #[serde(default)]
    pub epigraph: Option<String>,
    #[serde(default)]
    pub epigraph_source: Option<String>,
    #[serde(default)]
    pub copyright_line: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub target_words: Option<i32>,
    #[serde(default)]
    pub target_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectChapter {
    #[serde(default)]
    pub id: String,
    /// Empty for a chapter the writer never named.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub scenes: Vec<ProjectScene>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScene {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub pov: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub target_words: Option<i32>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub story_date: Option<String>,
    /// Where this scene's Markdown is in the zip; informational on import.
    #[serde(default)]
    pub file: Option<String>,
    /// Ids of the notebook entries this scene is linked to.
    #[serde(default)]
    pub notebook: Vec<String>,
    /// The scene's exact document.
    #[serde(default)]
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// The sketch's fields, as the notebook stores them.
    #[serde(default)]
    pub fields: Option<Value>,
}

/// A name that is safe in a zip path and still readable: letters and digits,
/// everything else collapsed to one hyphen.
fn slug(title: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.chars().count() >= 40 {
            break;
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        fallback.to_string()
    } else {
        out.to_string()
    }
}

/// Writes the zip. Sets each scene's `file` to the path it was written at.
pub fn write(project: &mut Project) -> std::io::Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut files: Vec<(String, String)> = Vec::new();
    for (c, chapter) in project.chapters.iter_mut().enumerate() {
        let folder = format!(
            "manuscript/{:02}-{}",
            c + 1,
            slug(&chapter.title, &format!("chapter-{}", c + 1))
        );
        for (s, scene) in chapter.scenes.iter_mut().enumerate() {
            let path = format!(
                "{folder}/{:02}-{}.md",
                s + 1,
                slug(&scene.title, &format!("scene-{}", s + 1))
            );
            files.push((path.clone(), markdown::write_doc(&scene.content)));
            scene.file = Some(path);
        }
    }

    zip.start_file("project.json", deflated)?;
    zip.write_all(&serde_json::to_vec_pretty(&*project).map_err(std::io::Error::other)?)?;
    zip.start_file("README.txt", deflated)?;
    zip.write_all(README.as_bytes())?;
    for (path, text) in files {
        zip.start_file(path, deflated)?;
        zip.write_all(text.as_bytes())?;
    }
    Ok(zip.finish()?.into_inner())
}

const README: &str = "This is a Just Type Studio project.\n\
\n\
project.json   The whole book: its details, chapters, scenes and notebook.\n\
               Import this zip into Just Type Studio to get the book back.\n\
manuscript/    Each scene as a Markdown file, one folder per chapter, for\n\
               reading or editing anywhere. Importing uses project.json when\n\
               it is present; edit the scene text there, or import the\n\
               Markdown files on their own.\n";

fn read_capped(file: &mut impl Read, remaining: &mut u64) -> Result<Vec<u8>, String> {
    let limit = MAX_ENTRY_BYTES.min(*remaining);
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("could not read the zip: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("that zip holds more than an import will read".into());
    }
    *remaining -= bytes.len() as u64;
    Ok(bytes)
}

/// Reads a project from a zip: `project.json` if it has one, else its
/// Markdown files.
pub fn read(bytes: &[u8]) -> Result<Project, String> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| "that is not a zip file".to_string())?;
    if archive.len() > MAX_ENTRIES {
        return Err("that zip has too many files".into());
    }
    let mut remaining = MAX_TOTAL_BYTES;

    let mut names: Vec<(usize, String)> = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|e| format!("could not read the zip: {e}"))?;
        if file.is_dir() {
            continue;
        }
        names.push((index, file.name().replace('\\', "/")));
    }

    if let Some((index, _)) = names.iter().find(|(_, n)| n == "project.json") {
        let mut file = archive.by_index(*index).map_err(|e| e.to_string())?;
        let json = read_capped(&mut file, &mut remaining)?;
        let project: Project = serde_json::from_slice(&json)
            .map_err(|e| format!("project.json is not a Just Type Studio project: {e}"))?;
        if project.format != FORMAT {
            return Err("project.json is not a Just Type Studio project".into());
        }
        if project.format_version > FORMAT_VERSION {
            return Err(
                "that project was made by a newer Just Type Studio — update this one to open it"
                    .into(),
            );
        }
        return Ok(project);
    }

    // No project.json: Markdown and text files, a chapter per folder.
    let mut texts: Vec<(usize, String)> = names
        .into_iter()
        .filter(|(_, n)| {
            let lower = n.to_ascii_lowercase();
            !n.starts_with("__MACOSX/")
                && !n.rsplit('/').next().unwrap_or("").starts_with('.')
                && (lower.ends_with(".md")
                    || lower.ends_with(".markdown")
                    || lower.ends_with(".txt"))
                && !lower.ends_with("readme.txt")
                && !lower.ends_with("readme.md")
        })
        .collect();
    if texts.is_empty() {
        return Err("that zip has no project.json and no Markdown or text files".into());
    }
    texts.sort_by(|a, b| a.1.cmp(&b.1));

    let mut chapters: Vec<ProjectChapter> = Vec::new();
    let mut last_folder: Option<String> = None;
    for (index, name) in texts {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let body = read_capped(&mut file, &mut remaining)?;
        let text = String::from_utf8_lossy(&body);
        let (folder, leaf) = match name.rsplit_once('/') {
            Some((folder, leaf)) => (folder.to_string(), leaf.to_string()),
            None => (String::new(), name.clone()),
        };
        if last_folder.as_deref() != Some(folder.as_str()) {
            let title = folder
                .rsplit('/')
                .next()
                .map(clean_name)
                .unwrap_or_default();
            chapters.push(ProjectChapter {
                title,
                ..Default::default()
            });
            last_folder = Some(folder);
        }
        let stem = leaf.rsplit_once('.').map(|(s, _)| s).unwrap_or(&leaf);
        chapters.last_mut().unwrap().scenes.push(ProjectScene {
            title: clean_name(stem),
            content: markdown::parse(&text),
            ..Default::default()
        });
    }
    Ok(Project {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        work: ProjectWork {
            title: FALLBACK_TITLE.into(),
            ..Default::default()
        },
        chapters,
        ..Default::default()
    })
}

/// "01-the-door" → "the door": the order prefix and the hyphens were the
/// file's, not the writer's.
pub fn clean_name(raw: &str) -> String {
    let trimmed = raw.trim_start_matches(|c: char| c.is_ascii_digit());
    let trimmed = trimmed.trim_start_matches(['-', '_', '.', ' ']);
    let base = if trimmed.is_empty() { raw } else { trimmed };
    base.replace(['-', '_'], " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project() -> Project {
        let para = |t: &str| json!({"type":"paragraph","content":[{"type":"text","text":t}]});
        Project {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            exported_at: "2026-10-09T00:00:00Z".into(),
            work: ProjectWork {
                title: "The Long Way Home".into(),
                pen_name: Some("A. Writer".into()),
                target_words: Some(80_000),
                ..Default::default()
            },
            chapters: vec![
                ProjectChapter {
                    id: "c1".into(),
                    title: "The Door".into(),
                    scenes: vec![ProjectScene {
                        id: "s1".into(),
                        title: String::new(),
                        status: Some("revised".into()),
                        notebook: vec!["e1".into()],
                        content: json!({"type":"doc","content":[para("Opening line.")]}),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ProjectChapter {
                    id: "c2".into(),
                    scenes: vec![ProjectScene {
                        id: "s2".into(),
                        title: "Later / after".into(),
                        content: json!({"type":"doc","content":[para("Second.")]}),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ],
            notebook: vec![ProjectEntry {
                id: "e1".into(),
                kind: "character".into(),
                name: "Mara".into(),
                fields: Some(json!({"age":"34"})),
                ..Default::default()
            }],
        }
    }

    #[test]
    fn a_project_comes_back_as_it_went() {
        let mut original = project();
        let zip = write(&mut original).unwrap();
        let back = read(&zip).unwrap();

        assert_eq!(back.work.title, "The Long Way Home");
        assert_eq!(back.work.pen_name.as_deref(), Some("A. Writer"));
        assert_eq!(back.work.target_words, Some(80_000));
        assert_eq!(back.chapters.len(), 2);
        // Unnamed stays unnamed: the placeholder is for showing, not keeping.
        assert_eq!(back.chapters[1].title, "");
        assert_eq!(back.chapters[0].scenes[0].title, "");
        assert_eq!(
            back.chapters[0].scenes[0].status.as_deref(),
            Some("revised")
        );
        assert_eq!(back.chapters[0].scenes[0].notebook, vec!["e1"]);
        assert_eq!(
            back.chapters[0].scenes[0].content,
            original.chapters[0].scenes[0].content
        );
        assert_eq!(back.notebook[0].fields, Some(json!({"age":"34"})));
    }

    #[test]
    fn each_scene_is_a_markdown_file_in_its_chapter_folder() {
        let mut original = project();
        let zip = write(&mut original).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "README.txt",
                "manuscript/01-the-door/01-scene-1.md",
                "manuscript/02-chapter-2/01-later-after.md",
                "project.json"
            ]
        );
        let mut text = String::new();
        archive
            .by_name("manuscript/01-the-door/01-scene-1.md")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "Opening line.\n");
    }

    #[test]
    fn a_zip_of_markdown_alone_imports_by_folder_and_file() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default();
        for (name, body) in [
            ("book/02-second/01-b.md", "Bee.\n"),
            ("book/01-first/01-a.md", "Ay *italic*.\n"),
            ("book/01-first/02-c.md", "Cee.\n"),
            ("__MACOSX/book/._x.md", "junk"),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        let back = read(&zip.finish().unwrap().into_inner()).unwrap();
        assert_eq!(back.chapters.len(), 2);
        assert_eq!(back.chapters[0].title, "first");
        assert_eq!(back.chapters[0].scenes.len(), 2);
        assert_eq!(back.chapters[0].scenes[0].title, "a");
        assert_eq!(back.chapters[1].scenes[0].title, "b");
    }

    #[test]
    fn what_is_not_a_project_is_refused_plainly() {
        assert!(read(b"not a zip").unwrap_err().contains("not a zip"));

        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("project.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"format":"something-else","formatVersion":1,"work":{"title":"x"}}"#)
            .unwrap();
        let err = read(&zip.finish().unwrap().into_inner()).unwrap_err();
        assert!(err.contains("not a Just Type Studio project"), "{err}");

        let mut newer = project();
        newer.format_version = FORMAT_VERSION + 1;
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("project.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&newer).unwrap()).unwrap();
        let err = read(&zip.finish().unwrap().into_inner()).unwrap_err();
        assert!(err.contains("newer"), "{err}");
    }

    #[test]
    fn names_lose_their_order_prefix() {
        assert_eq!(clean_name("01-the-door"), "the door");
        assert_eq!(clean_name("12_Chapter_Two"), "Chapter Two");
        assert_eq!(clean_name("2024"), "2024");
    }
}
