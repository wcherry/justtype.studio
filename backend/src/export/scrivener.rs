//! A Scrivener project (`.scriv`), zipped, as a project to import.
//!
//! ```text
//! My Novel.scriv/
//!   My Novel.scrivx                   the binder, as XML
//!   Files/Data/<UUID>/content.rtf     Scrivener 3: each document's text
//!   Files/Data/<UUID>/synopsis.txt    …and its index-card synopsis
//!   Files/Docs/<ID>.rtf               Scrivener 2: the same, by number
//!   Files/Docs/<ID>_synopsis.txt
//! ```
//!
//! The Draft (Manuscript) folder becomes the book. Each item at its top level
//! is a chapter: a folder (or a document with documents under it) is a chapter
//! of its own text and everything beneath it, flattened in binder order; a lone
//! document is a chapter of one scene. Synopses become summaries, and a status
//! Scrivener's default list knows becomes the scene's status.
//!
//! The other folders become the notebook, a document per entry: those under a
//! folder called Characters are characters, Places (or Settings, Locations)
//! places, and everything else, research included, notes. The Trash, template
//! sheets and compile-only front and back matter are left behind, as are
//! snapshots, document notes, keywords, labels and media.
use std::collections::HashMap;
use std::io::Read;

use roxmltree::{Document, Node};
use zip::ZipArchive;

use super::project::{
    read_capped, Project, ProjectChapter, ProjectEntry, ProjectScene, ProjectWork, FORMAT,
    FORMAT_VERSION,
};
use super::rtf;
use crate::domain::doc;

/// Whether a zip's file names say it is a Scrivener project.
pub fn is_scrivener(names: &[(usize, String)]) -> bool {
    scrivx(names).is_some()
}

/// The binder file: the shallowest `.scrivx`, outside any macOS metadata.
fn scrivx(names: &[(usize, String)]) -> Option<&(usize, String)> {
    names
        .iter()
        .filter(|(_, n)| {
            !n.starts_with("__MACOSX/")
                && n.to_ascii_lowercase().ends_with(".scrivx")
                && !n.rsplit('/').next().unwrap_or("").starts_with('.')
        })
        .min_by_key(|(_, n)| n.matches('/').count())
}

struct Files<'a, R: Read + std::io::Seek> {
    archive: &'a mut ZipArchive<R>,
    index: HashMap<String, usize>,
    remaining: &'a mut u64,
}

impl<R: Read + std::io::Seek> Files<'_, R> {
    fn get(&mut self, path: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(&index) = self.index.get(path) else {
            return Ok(None);
        };
        let mut file = self.archive.by_index(index).map_err(|e| e.to_string())?;
        read_capped(&mut file, self.remaining).map(Some)
    }
}

/// One binder item, with only what an import uses.
struct Item {
    kind: String,
    title: String,
    /// Where its text and synopsis are, without the extension: `Files/Data/
    /// <UUID>/` for Scrivener 3, `Files/Docs/<ID>` for Scrivener 2.
    uuid: Option<String>,
    id: Option<String>,
    status: Option<String>,
    children: Vec<Item>,
}

fn item(node: Node) -> Item {
    let child_text = |name: &str| {
        node.children()
            .find(|c| c.has_tag_name(name))
            .and_then(|c| c.text())
            .map(|t| t.trim().to_string())
    };
    let meta = node.children().find(|c| c.has_tag_name("MetaData"));
    Item {
        kind: node.attribute("Type").unwrap_or("Text").to_string(),
        title: child_text("Title").unwrap_or_default(),
        uuid: node.attribute("UUID").map(str::to_string),
        id: node.attribute("ID").map(str::to_string),
        status: meta
            .and_then(|m| m.children().find(|c| c.has_tag_name("StatusID")))
            .and_then(|s| s.text())
            .map(|s| s.trim().to_string()),
        children: node
            .children()
            .find(|c| c.has_tag_name("Children"))
            .map(|c| {
                c.children()
                    .filter(|c| c.has_tag_name("BinderItem"))
                    .map(item)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

impl Item {
    fn is_folder(&self) -> bool {
        self.kind.ends_with("Folder") || self.kind == "Root"
    }

    /// Text, folder and the binder's own folders hold writing; PDFs, images
    /// and web pages do not.
    fn holds_text(&self) -> bool {
        self.is_folder() || self.kind == "Text"
    }
}

/// A document's text and synopsis, read from the project.
struct Texts {
    content: Option<serde_json::Value>,
    synopsis: Option<String>,
}

fn texts<R: Read + std::io::Seek>(
    files: &mut Files<R>,
    root: &str,
    item: &Item,
) -> Result<Texts, String> {
    let (content, synopsis) = if let Some(uuid) = &item.uuid {
        let base = format!("{root}Files/Data/{uuid}/");
        (
            files.get(&format!("{base}content.rtf"))?,
            files.get(&format!("{base}synopsis.txt"))?,
        )
    } else if let Some(id) = &item.id {
        let base = format!("{root}Files/Docs/{id}");
        (
            files.get(&format!("{base}.rtf"))?,
            files.get(&format!("{base}_synopsis.txt"))?,
        )
    } else {
        (None, None)
    };
    Ok(Texts {
        content: content
            .map(|bytes| rtf::parse(&bytes))
            .filter(|d| doc::word_count(d) > 0),
        synopsis: synopsis
            .map(|bytes| String::from_utf8_lossy(&bytes).trim().to_string())
            .filter(|s| !s.is_empty()),
    })
}

/// Scrivener's statuses are the writer's to rename; the ones it ships with,
/// and the obvious variations, map to ours.
fn status_for(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    if name.contains("final") || name.contains("done") || name.contains("complete") {
        Some("done")
    } else if name.contains("revis") || name.contains("second") {
        Some("revised")
    } else if name.contains("draft") {
        Some("drafting")
    } else if name.contains("to do") || name.contains("todo") || name.contains("outline") {
        Some("outline")
    } else {
        None
    }
}

struct Reader<'a, 'f, R: Read + std::io::Seek> {
    files: &'a mut Files<'f, R>,
    root: String,
    statuses: HashMap<String, &'static str>,
}

impl<R: Read + std::io::Seek> Reader<'_, '_, R> {
    fn scene(&mut self, item: &Item, title: &str, texts: Texts) -> ProjectScene {
        ProjectScene {
            title: title.to_string(),
            summary: texts.synopsis,
            status: item
                .status
                .as_ref()
                .and_then(|s| self.statuses.get(s))
                .map(|s| s.to_string()),
            content: texts.content.unwrap_or_else(doc::empty_doc),
            ..Default::default()
        }
    }

    /// Every document at and under `item`, in binder order. A folder with no
    /// text of its own is only structure and makes no scene.
    fn flatten(&mut self, item: &Item, out: &mut Vec<ProjectScene>) -> Result<(), String> {
        if !item.holds_text() {
            return Ok(());
        }
        let texts = texts(self.files, &self.root, item)?;
        if !item.is_folder() || texts.content.is_some() {
            out.push(self.scene(item, &item.title, texts));
        }
        for child in &item.children {
            self.flatten(child, out)?;
        }
        Ok(())
    }

    fn chapter(&mut self, item: &Item) -> Result<Option<ProjectChapter>, String> {
        if !item.holds_text() {
            return Ok(None);
        }
        let texts = texts(self.files, &self.root, item)?;
        let mut chapter = ProjectChapter {
            title: item.title.clone(),
            ..Default::default()
        };
        if item.children.is_empty() && !item.is_folder() {
            // A lone document is the whole chapter; its name is the chapter's.
            chapter.scenes.push(self.scene(item, "", texts));
            return Ok(Some(chapter));
        }
        if item.is_folder() && texts.content.is_none() {
            chapter.summary = texts.synopsis;
        } else {
            chapter.scenes.push(self.scene(item, &item.title, texts));
        }
        for child in &item.children {
            self.flatten(child, &mut chapter.scenes)?;
        }
        Ok(Some(chapter))
    }

    /// Each document under `item` that has anything in it, as an entry of
    /// `kind`. Folders give their kind to what is inside them.
    fn notebook(
        &mut self,
        item: &Item,
        kind: &'static str,
        out: &mut Vec<ProjectEntry>,
    ) -> Result<(), String> {
        if !item.holds_text() {
            return Ok(());
        }
        let kind = kind_for(&item.title).unwrap_or(kind);
        let texts = texts(self.files, &self.root, item)?;
        if texts.content.is_some() || (!item.is_folder() && texts.synopsis.is_some()) {
            out.push(ProjectEntry {
                id: format!("scrivener-{}", out.len()),
                kind: kind.into(),
                name: if item.title.is_empty() {
                    "Untitled".into()
                } else {
                    item.title.clone()
                },
                summary: texts.synopsis,
                body: texts
                    .content
                    .map(|d| doc::plain_text(&d).replace('\n', "\n\n")),
                fields: None,
            });
        }
        for child in &item.children {
            self.notebook(child, kind, out)?;
        }
        Ok(())
    }
}

/// What a folder's name says its documents are.
fn kind_for(folder: &str) -> Option<&'static str> {
    match folder.trim().to_lowercase().as_str() {
        "characters" | "character" | "people" | "cast" => Some("character"),
        "places" | "place" | "settings" | "setting" | "locations" | "location" => Some("place"),
        _ => None,
    }
}

/// Top-level folders that are Scrivener's machinery rather than the writer's
/// material.
fn left_behind(item: &Item) -> bool {
    item.kind == "TrashFolder"
        || matches!(
            item.title.trim().to_lowercase().as_str(),
            "template sheets" | "templates" | "front matter" | "back matter"
        )
}

/// Reads the Scrivener project in a zip whose file `names` it has already
/// listed.
pub fn read<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    names: &[(usize, String)],
    remaining: &mut u64,
) -> Result<Project, String> {
    let (binder_index, binder_name) = scrivx(names)
        .cloned()
        .ok_or_else(|| "that is not a Scrivener project".to_string())?;
    let root = match binder_name.rsplit_once('/') {
        Some((folder, _)) => format!("{folder}/"),
        None => String::new(),
    };

    let xml = {
        let mut file = archive.by_index(binder_index).map_err(|e| e.to_string())?;
        let bytes = read_capped(&mut file, remaining)?;
        String::from_utf8(bytes).map_err(|_| "that Scrivener project's binder is not text")?
    };
    let xml = xml.trim_start_matches('\u{feff}');
    let parsed = Document::parse(xml)
        .map_err(|e| format!("that Scrivener project's binder could not be read: {e}"))?;
    let project = parsed.root_element();
    if !project.has_tag_name("ScrivenerProject") {
        return Err("that is not a Scrivener project".into());
    }

    let binder: Vec<Item> = project
        .children()
        .find(|c| c.has_tag_name("Binder"))
        .map(|b| {
            b.children()
                .filter(|c| c.has_tag_name("BinderItem"))
                .map(item)
                .collect()
        })
        .unwrap_or_default();
    let statuses: HashMap<String, &'static str> = project
        .descendants()
        .filter(|n| n.has_tag_name("Status"))
        .filter_map(|n| Some((n.attribute("ID")?.to_string(), status_for(n.text()?)?)))
        .collect();
    let title = project
        .descendants()
        .find(|n| n.has_tag_name("ProjectTitle"))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            let folder = root.trim_end_matches('/').rsplit('/').next().unwrap_or("");
            let name = if folder.is_empty() {
                binder_name.rsplit('/').next().unwrap_or("")
            } else {
                folder
            };
            let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
            if stem.trim().is_empty() {
                "Imported project".into()
            } else {
                stem.trim().to_string()
            }
        });

    let index: HashMap<String, usize> = names.iter().map(|(i, n)| (n.clone(), *i)).collect();
    let mut files = Files {
        archive,
        index,
        remaining,
    };
    let mut reader = Reader {
        files: &mut files,
        root,
        statuses,
    };

    let draft = binder
        .iter()
        .find(|i| i.kind == "DraftFolder")
        .ok_or_else(|| {
            "that Scrivener project has no Draft folder to make a book from".to_string()
        })?;
    let mut chapters = Vec::new();
    for item in &draft.children {
        if let Some(chapter) = reader.chapter(item)? {
            chapters.push(chapter);
        }
    }

    let mut notebook = Vec::new();
    for item in &binder {
        if item.kind == "DraftFolder" || left_behind(item) {
            continue;
        }
        reader.notebook(item, "note", &mut notebook)?;
    }

    Ok(Project {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION,
        work: ProjectWork {
            title,
            ..Default::default()
        },
        chapters,
        notebook,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    use super::super::project;

    fn rtf(text: &str) -> String {
        format!("{{\\rtf1\\ansi\\ansicpg1252{{\\fonttbl\\f0 Palatino;}}\\f0 {text}}}")
    }

    fn zip(files: &[(&str, String)]) -> Vec<u8> {
        let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in files {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    const BINDER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ScrivenerProject Version="2.0" Identifier="X">
  <Binder>
    <BinderItem UUID="D" Type="DraftFolder"><Title>Manuscript</Title><Children>
      <BinderItem UUID="C1" Type="Folder"><Title>Chapter One</Title><Children>
        <BinderItem UUID="S1" Type="Text"><Title>The Door</Title>
          <MetaData><StatusID>2</StatusID></MetaData></BinderItem>
        <BinderItem UUID="P1" Type="Folder"><Title>Part</Title><Children>
          <BinderItem UUID="S2" Type="Text"><Title>Nested</Title></BinderItem>
        </Children></BinderItem>
        <BinderItem UUID="IMG" Type="Image"><Title>Map</Title></BinderItem>
      </Children></BinderItem>
      <BinderItem UUID="C2" Type="Text"><Title>Chapter Two</Title></BinderItem>
    </Children></BinderItem>
    <BinderItem UUID="R" Type="ResearchFolder"><Title>Research</Title><Children>
      <BinderItem UUID="N1" Type="Text"><Title>Ships</Title></BinderItem>
    </Children></BinderItem>
    <BinderItem UUID="CH" Type="Folder"><Title>Characters</Title><Children>
      <BinderItem UUID="M" Type="Text"><Title>Mara</Title></BinderItem>
      <BinderItem UUID="E" Type="Text"><Title>Empty</Title></BinderItem>
    </Children></BinderItem>
    <BinderItem UUID="T" Type="Folder"><Title>Template Sheets</Title><Children>
      <BinderItem UUID="TS" Type="Text"><Title>Character Sketch</Title></BinderItem>
    </Children></BinderItem>
    <BinderItem UUID="X" Type="TrashFolder"><Title>Trash</Title><Children>
      <BinderItem UUID="GONE" Type="Text"><Title>Deleted</Title></BinderItem>
    </Children></BinderItem>
  </Binder>
  <StatusSettings><StatusItems>
    <Status ID="0">To Do</Status><Status ID="2">Revised Draft</Status>
  </StatusItems></StatusSettings>
</ScrivenerProject>"#;

    fn scrivener3() -> Vec<u8> {
        let data = |uuid: &str, file: &str| format!("The Long Way.scriv/Files/Data/{uuid}/{file}");
        let files = vec![
            (
                "The Long Way.scriv/The Long Way.scrivx".to_string(),
                BINDER.to_string(),
            ),
            (data("C1", "synopsis.txt"), "Mara comes home.\n".into()),
            (
                data("S1", "content.rtf"),
                rtf("The door was \\i open\\i0 ."),
            ),
            (data("S1", "synopsis.txt"), "She arrives.".into()),
            (data("S2", "content.rtf"), rtf("Deeper in.")),
            (data("C2", "content.rtf"), rtf("All of chapter two.")),
            (data("N1", "content.rtf"), rtf("Clinker-built.\\par Oars.")),
            (data("M", "content.rtf"), rtf("Thirty-four.")),
            (data("TS", "content.rtf"), rtf("Name: Role:")),
            (data("GONE", "content.rtf"), rtf("Cut.")),
            (
                "__MACOSX/The Long Way.scriv/._x.scrivx".to_string(),
                "junk".into(),
            ),
        ];
        let named: Vec<(&str, String)> =
            files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
        zip(&named)
    }

    #[test]
    fn the_draft_becomes_the_book_and_the_rest_the_notebook() {
        let p = project::read(&scrivener3()).unwrap();
        assert_eq!(p.work.title, "The Long Way");

        assert_eq!(p.chapters.len(), 2);
        let one = &p.chapters[0];
        assert_eq!(one.title, "Chapter One");
        assert_eq!(one.summary.as_deref(), Some("Mara comes home."));
        let titles: Vec<&str> = one.scenes.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            ["The Door", "Nested"],
            "folders and images make no scene"
        );
        assert_eq!(one.scenes[0].summary.as_deref(), Some("She arrives."));
        assert_eq!(one.scenes[0].status.as_deref(), Some("revised"));
        assert_eq!(
            one.scenes[0].content["content"][0]["content"][1]["marks"][0]["type"],
            "italic"
        );

        let two = &p.chapters[1];
        assert_eq!(two.title, "Chapter Two");
        assert_eq!(two.scenes.len(), 1);
        assert_eq!(two.scenes[0].title, "");
        assert_eq!(
            super::doc::plain_text(&two.scenes[0].content),
            "All of chapter two."
        );

        let entries: Vec<(&str, &str)> = p
            .notebook
            .iter()
            .map(|e| (e.kind.as_str(), e.name.as_str()))
            .collect();
        assert_eq!(entries, [("note", "Ships"), ("character", "Mara")]);
        assert_eq!(
            p.notebook[0].body.as_deref(),
            Some("Clinker-built.\n\nOars.")
        );
    }

    #[test]
    fn a_scrivener_2_project_reads_by_number() {
        let binder = r#"<ScrivenerProject Version="1.5"><Binder>
          <BinderItem ID="0" Type="DraftFolder"><Title>Draft</Title><Children>
            <BinderItem ID="3" Type="Text"><Title>Opening</Title></BinderItem>
          </Children></BinderItem>
        </Binder><ProjectProperties><ProjectTitle>Old Book</ProjectTitle></ProjectProperties>
        </ScrivenerProject>"#;
        let bytes = zip(&[
            ("Old.scrivx", binder.into()),
            ("Files/Docs/3.rtf", rtf("It began.")),
            ("Files/Docs/3_synopsis.txt", "The start.".into()),
        ]);
        let p = project::read(&bytes).unwrap();
        assert_eq!(p.work.title, "Old Book");
        assert_eq!(p.chapters[0].title, "Opening");
        assert_eq!(
            p.chapters[0].scenes[0].summary.as_deref(),
            Some("The start.")
        );
        assert_eq!(
            super::doc::plain_text(&p.chapters[0].scenes[0].content),
            "It began."
        );
    }

    #[test]
    fn a_binder_that_is_not_scrivener_is_refused_plainly() {
        let bytes = zip(&[("x.scrivx", "<html/>".into())]);
        assert!(project::read(&bytes)
            .unwrap_err()
            .contains("not a Scrivener project"));
        let bytes = zip(&[(
            "x.scrivx",
            "<ScrivenerProject><Binder/></ScrivenerProject>".into(),
        )]);
        assert!(project::read(&bytes)
            .unwrap_err()
            .contains("no Draft folder"));
        let bytes = zip(&[("x.scrivx", "<ScrivenerProject".into())]);
        assert!(project::read(&bytes)
            .unwrap_err()
            .contains("could not be read"));
    }
}
