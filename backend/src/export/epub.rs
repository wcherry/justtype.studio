//! EPUB 3 output, for beta readers who would rather read on an e-reader than in
//! a browser.
//!
//! The container is a ZIP with one rule that matters: `mimetype` must be the
//! first entry and stored uncompressed, or readers reject the file before
//! opening anything else.
//!
//! The book is assembled as a list of parts first and checked — every spine
//! entry in the manifest, every navigation target a real file — before any of
//! it is zipped. `epubcheck` is the real validator, but it is a Java tool and
//! stays out of the runtime image; these checks catch the structural mistakes
//! this writer could make.
use std::collections::HashSet;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::options::ExportOptions;
use super::web::{epub_type, style_overrides, NoteMarkup};
use super::{xml_escape, Block, ChapterContent, Manuscript, NoteLayout, Span};

/// One file in the package.
struct Part {
    path: String,
    id: String,
    media_type: &'static str,
    properties: Option<&'static str>,
    body: Vec<u8>,
    spine: bool,
}

impl Part {
    fn text(path: &str, id: &str, media_type: &'static str, body: String) -> Self {
        Self {
            path: path.into(),
            id: id.into(),
            media_type,
            properties: None,
            body: body.into_bytes(),
            spine: false,
        }
    }
    fn page(path: &str, id: &str, body: String) -> Self {
        Self {
            spine: true,
            ..Self::text(path, id, "application/xhtml+xml", body)
        }
    }
}

/// A target in the navigation document.
struct NavEntry {
    href: String,
    title: String,
}

pub fn write(manuscript: &Manuscript, options: &ExportOptions) -> std::io::Result<Vec<u8>> {
    let m = manuscript;
    let epub = options.epub();
    let split = epub.split_chapters;
    let book_notes = m.note_layout == NoteLayout::EndnotesBook
        && m.chapters.iter().any(|c| !c.notes.is_empty());

    let chapter_href = |index: usize| -> String {
        if split {
            format!("chapter{}.xhtml", index + 1)
        } else {
            "content.xhtml".to_string()
        }
    };

    let mut parts: Vec<Part> = Vec::new();
    let mut nav: Vec<NavEntry> = Vec::new();

    let overrides = style_overrides(&options.style.resolve(), "h1", true);
    let mut css = STYLESHEET.to_string();
    if m.chapters.iter().any(|c| !c.notes.is_empty()) {
        css.push_str(NOTES_STYLE);
    }
    css.push_str(&overrides);
    parts.push(Part::text("OEBPS/style.css", "css", "text/css", css));

    if let Some(cover) = &m.cover {
        let media = if cover.mime == "image/png" {
            "image/png"
        } else {
            "image/jpeg"
        };
        parts.push(Part {
            path: format!("OEBPS/cover.{}", cover.extension()),
            id: "cover-image".into(),
            media_type: media,
            properties: Some("cover-image"),
            body: cover.bytes.clone(),
            spine: false,
        });
        if epub.cover_page {
            parts.push(Part::page(
                "OEBPS/cover.xhtml",
                "cover",
                page(
                    m,
                    "Cover",
                    &format!(
                        "<div class=\"cover\"><img src=\"cover.{}\" alt=\"Cover of {}\"/></div>",
                        cover.extension(),
                        xml_escape(&m.title)
                    ),
                    Some("cover"),
                ),
            ));
        }
    }

    if !m.omit_front_matter {
        parts.push(Part::page(
            "OEBPS/title.xhtml",
            "title",
            title_page(m),
        ));
    }

    if split {
        for (index, chapter) in m.chapters.iter().enumerate() {
            let body = chapter_markup(m, index, chapter, false, None, &chapter_href);
            parts.push(Part::page(
                &format!("OEBPS/chapter{}.xhtml", index + 1),
                &format!("chapter{}", index + 1),
                page(m, &chapter.title, &body, Some(epub_type(chapter))),
            ));
            nav.push(NavEntry {
                href: chapter_href(index),
                title: chapter.title.clone(),
            });
        }
    } else {
        let body: String = m
            .chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| {
                chapter_markup(m, index, chapter, true, Some(index), &chapter_href)
            })
            .collect();
        parts.push(Part::page(
            "OEBPS/content.xhtml",
            "content",
            page(m, &m.title, &body, None),
        ));
        for (index, chapter) in m.chapters.iter().enumerate() {
            nav.push(NavEntry {
                href: format!("content.xhtml#chapter-{}", index + 1),
                title: chapter.title.clone(),
            });
        }
    }

    if book_notes {
        let markup = NoteMarkup {
            scope: String::new(),
            notes_page: None,
            epub: true,
            classed: true,
        };
        let scopes: Vec<String> = (0..m.chapters.len()).map(|i| format!("c{}", i + 1)).collect();
        let all: Vec<(&str, &super::Note)> = m
            .chapters
            .iter()
            .enumerate()
            .flat_map(|(i, c)| c.notes.iter().map(move |n| (i, n)))
            .map(|(i, n)| (scopes[i].as_str(), n))
            .collect();
        let body = markup.section(&all, m.note_label, |scope| {
            let index = scope.trim_start_matches('c').parse::<usize>().unwrap_or(1) - 1;
            chapter_href(index).split('#').next().unwrap_or("").to_string()
        });
        parts.push(Part::page(
            "OEBPS/notes.xhtml",
            "notes",
            page(m, "Notes", &body, Some("endnotes")),
        ));
        nav.push(NavEntry {
            href: "notes.xhtml".into(),
            title: "Notes".into(),
        });
    }

    parts.push(Part {
        properties: Some("nav"),
        ..Part::text(
            "OEBPS/nav.xhtml",
            "nav",
            "application/xhtml+xml",
            navigation(m, &nav, &parts),
        )
    });
    let opf = package(m, &parts);
    parts.push(Part::text(
        "OEBPS/content.opf",
        "opf",
        "application/oebps-package+xml",
        opf,
    ));

    verify(&parts, &nav)?;

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // First and uncompressed, per the specification.
    zip.start_file("mimetype", stored)?;
    zip.write_all(b"application/epub+zip")?;
    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(CONTAINER.as_bytes())?;
    // The old layout wrote the stylesheet and title first; order inside the
    // archive does not matter beyond `mimetype`.
    for part in &parts {
        zip.start_file(&part.path, deflated)?;
        zip.write_all(&part.body)?;
    }
    Ok(zip.finish()?.into_inner())
}

/// The structural checks: nothing the package points at is missing.
fn verify(parts: &[Part], nav: &[NavEntry]) -> std::io::Result<()> {
    let bad = |msg: String| Err(std::io::Error::other(format!("EPUB self-check: {msg}")));
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for part in parts {
        if !ids.insert(part.id.as_str()) {
            return bad(format!("two manifest items share the id `{}`", part.id));
        }
        paths.insert(part.path.trim_start_matches("OEBPS/").to_string());
    }
    if !parts.iter().any(|p| p.spine) {
        return bad("the spine is empty".into());
    }
    for entry in nav {
        let target = entry.href.split('#').next().unwrap_or("");
        if !paths.contains(target) {
            return bad(format!(
                "the navigation points at `{target}`, which is not in the package"
            ));
        }
    }
    Ok(())
}

pub(super) fn spans_to_html(spans: &[Span]) -> String {
    spans_html(spans, None)
}

/// Runs as markup; a note reference becomes a superscript link when the page
/// has somewhere to point it.
pub(super) fn spans_html(spans: &[Span], notes: Option<&super::web::NoteMarkup>) -> String {
    spans
        .iter()
        .map(|span| {
            if let Some(number) = span.note {
                return notes.map(|n| n.reference(number)).unwrap_or_default();
            }
            let text = xml_escape(&span.text).replace('\n', "<br/>");
            let mut html = text;
            if span.italic {
                html = format!("<em>{html}</em>");
            }
            if span.bold {
                html = format!("<strong>{html}</strong>");
            }
            if span.underline {
                html = format!("<u>{html}</u>");
            }
            html
        })
        .collect()
}

/// The markup of one chapter. In a single-file book each is a section with an
/// anchor the navigation can reach; in a split book the page is the chapter.
fn chapter_markup(
    m: &Manuscript,
    index: usize,
    chapter: &ChapterContent,
    section: bool,
    _anchor: Option<usize>,
    chapter_href: &dyn Fn(usize) -> String,
) -> String {
    let markup = NoteMarkup {
        scope: format!("c{}", index + 1),
        notes_page: (m.note_layout == NoteLayout::EndnotesBook).then(|| "notes.xhtml".to_string()),
        epub: true,
        classed: true,
    };
    let mut body = if section {
        format!(
            "<section epub:type=\"{}\" id=\"chapter-{}\">\n",
            epub_type(chapter),
            index + 1
        )
    } else {
        String::new()
    };
    body.push_str(&format!("<h1>{}</h1>\n", xml_escape(&chapter.title)));
    for (scene_index, scene) in chapter.scenes.iter().enumerate() {
        if scene_index > 0 {
            body.push_str("<hr class=\"scene\"/>\n");
        }
        // `first` suppresses the indent on the paragraph after a break, which is
        // the typographic convention in a printed book.
        let mut first = true;
        for block in &scene.blocks {
            let html = match block {
                Block::Paragraph(spans) => {
                    let class = if first { " class=\"first\"" } else { "" };
                    first = false;
                    format!("<p{class}>{}</p>", spans_html(spans, Some(&markup)))
                }
                Block::Heading { level, spans } => {
                    first = true;
                    let level = (*level).clamp(2, 6);
                    format!("<h{level}>{}</h{level}>", spans_html(spans, Some(&markup)))
                }
                Block::Quote(spans) => {
                    first = true;
                    format!(
                        "<blockquote><p>{}</p></blockquote>",
                        spans_html(spans, Some(&markup))
                    )
                }
                Block::SceneBreak => {
                    first = true;
                    "<hr class=\"scene\"/>".to_string()
                }
            };
            body.push_str(&html);
            body.push('\n');
        }
    }
    if m.note_layout != NoteLayout::EndnotesBook && !chapter.notes.is_empty() {
        let scope = markup.scope.clone();
        let refs: Vec<(&str, &super::Note)> =
            chapter.notes.iter().map(|n| (scope.as_str(), n)).collect();
        // Back-links stay on the page (or in the file) the mark is on.
        let here = chapter_href(index).split('#').next().unwrap_or("").to_string();
        body.push_str(&markup.section(&refs, m.note_label, |_| {
            if section {
                String::new()
            } else {
                here.clone()
            }
        }));
    }
    if section {
        body.push_str("</section>\n");
    }
    body
}

fn title_page(manuscript: &Manuscript) -> String {
    let mut body = format!(
        "<div class=\"title-page\">\n<h1>{}</h1>\n",
        xml_escape(&manuscript.title)
    );
    if let Some(subtitle) = &manuscript.subtitle {
        body.push_str(&format!(
            "<p class=\"subtitle\">{}</p>\n",
            xml_escape(subtitle)
        ));
    }
    body.push_str(&format!(
        "<p class=\"byline\">{}</p>\n",
        xml_escape(&manuscript.byline)
    ));
    if let Some(line) = &manuscript.copyright_line {
        body.push_str(&format!(
            "<p class=\"copyright\">{}</p>\n",
            xml_escape(line)
        ));
    }
    if let Some(dedication) = &manuscript.dedication {
        body.push_str(&format!(
            "<p class=\"dedication\">{}</p>\n",
            xml_escape(dedication)
        ));
    }
    if let Some(epigraph) = &manuscript.epigraph {
        body.push_str(&format!(
            "<blockquote class=\"epigraph\"><p>{}</p>",
            xml_escape(epigraph)
        ));
        if let Some(source) = &manuscript.epigraph_source {
            body.push_str(&format!("<p class=\"source\">— {}</p>", xml_escape(source)));
        }
        body.push_str("</blockquote>\n");
    }
    body.push_str("</div>");
    page(manuscript, &manuscript.title, &body, Some("titlepage"))
}

fn page(m: &Manuscript, title: &str, body: &str, kind: Option<&str>) -> String {
    let lang = xml_escape(m.lang());
    let dir = if m.rtl { " dir=\"rtl\"" } else { "" };
    let kind = kind
        .map(|k| format!(" epub:type=\"{k}\""))
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="{lang}" xml:lang="{lang}"{dir}>
<head><title>{}</title><link rel="stylesheet" type="text/css" href="style.css"/></head>
<body{kind}>
{body}
</body>
</html>"#,
        xml_escape(title)
    )
}

fn navigation(m: &Manuscript, entries: &[NavEntry], parts: &[Part]) -> String {
    let items: String = entries
        .iter()
        .map(|e| {
            format!(
                "      <li><a href=\"{}\">{}</a></li>\n",
                e.href,
                xml_escape(&e.title)
            )
        })
        .collect();
    let lang = xml_escape(m.lang());
    let dir = if m.rtl { " dir=\"rtl\"" } else { "" };
    // The contents list is always written, since EPUB 3 requires it; when the
    // writer asked for no table of contents it is simply not shown.
    let hidden = if m.toc { "" } else { " hidden=\"hidden\"" };

    let mut landmarks = String::new();
    if parts.iter().any(|p| p.path.ends_with("cover.xhtml")) {
        landmarks.push_str("      <li><a epub:type=\"cover\" href=\"cover.xhtml\">Cover</a></li>\n");
    }
    if let Some(first) = entries.first() {
        landmarks.push_str(&format!(
            "      <li><a epub:type=\"bodymatter\" href=\"{}\">Start of the book</a></li>\n",
            first.href
        ));
    }
    let landmarks = if landmarks.is_empty() {
        String::new()
    } else {
        format!(
            "  <nav epub:type=\"landmarks\" hidden=\"hidden\">\n    <h2>Guide</h2>\n    <ol>\n{landmarks}    </ol>\n  </nav>\n"
        )
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="{lang}" xml:lang="{lang}"{dir}>
<head><title>Contents</title></head>
<body>
  <nav epub:type="toc" id="toc"{hidden}>
    <h1>Contents</h1>
    <ol>
{items}    </ol>
  </nav>
{landmarks}</body>
</html>"#
    )
}

/// A UUID derived from a seed (version 8, "custom": the bits are ours), so
/// the same input always names the same package.
fn stable_uuid(seed: &str) -> uuid::Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

/// MARC relator codes for the roles a contributor can have.
fn relator(role: &str) -> &'static str {
    match role {
        "editor" => "edt",
        "translator" => "trl",
        "illustrator" => "ill",
        "co-author" => "aut",
        _ => "ctb",
    }
}

fn package(m: &Manuscript, parts: &[Part]) -> String {
    let manifest: String = parts
        .iter()
        .filter(|p| p.id != "opf")
        .map(|p| {
            let props = p
                .properties
                .map(|v| format!(" properties=\"{v}\""))
                .unwrap_or_default();
            format!(
                "    <item id=\"{}\" href=\"{}\" media-type=\"{}\"{props}/>\n",
                p.id,
                p.path.trim_start_matches("OEBPS/"),
                p.media_type
            )
        })
        .collect();
    let spine: String = parts
        .iter()
        .filter(|p| p.spine)
        .map(|p| format!("    <itemref idref=\"{}\"/>\n", p.id))
        .collect();

    let identifier = match (&m.identifier, &m.frozen_id) {
        (Some(id), _) => xml_escape(id),
        // The same version gives the same package id, so it exports to the
        // same bytes every time.
        (None, Some(seed)) => format!("urn:uuid:{}", stable_uuid(seed)),
        (None, None) => format!("urn:uuid:{}", uuid::Uuid::new_v4()),
    };
    let mut meta = format!(
        "    <dc:identifier id=\"book-id\">{identifier}</dc:identifier>\n\
    <dc:title>{}</dc:title>\n\
    <dc:creator>{}</dc:creator>\n\
    <dc:language>{}</dc:language>\n",
        xml_escape(&m.title),
        xml_escape(&m.byline),
        xml_escape(m.lang()),
    );
    if let Some(description) = &m.description {
        meta.push_str(&format!(
            "    <dc:description>{}</dc:description>\n",
            xml_escape(description)
        ));
    }
    if let Some(publisher) = &m.publisher {
        meta.push_str(&format!(
            "    <dc:publisher>{}</dc:publisher>\n",
            xml_escape(publisher)
        ));
    }
    if let Some(date) = &m.published {
        meta.push_str(&format!("    <dc:date>{}</dc:date>\n", xml_escape(date)));
    }
    for keyword in &m.keywords {
        meta.push_str(&format!(
            "    <dc:subject>{}</dc:subject>\n",
            xml_escape(keyword)
        ));
    }
    for (index, contributor) in m.contributors.iter().enumerate() {
        meta.push_str(&format!(
            "    <dc:contributor id=\"contrib{n}\">{}</dc:contributor>\n\
    <meta refines=\"#contrib{n}\" property=\"role\" scheme=\"marc:relators\">{}</meta>\n",
            xml_escape(&contributor.name),
            relator(&contributor.role),
            n = index + 1
        ));
    }
    if m.cover.is_some() {
        // Older readers find the cover through this rather than the property.
        meta.push_str("    <meta name=\"cover\" content=\"cover-image\"/>\n");
    }
    meta.push_str(
        "    <meta property=\"schema:accessMode\">textual</meta>\n\
    <meta property=\"schema:accessModeSufficient\">textual</meta>\n\
    <meta property=\"schema:accessibilityFeature\">tableOfContents</meta>\n\
    <meta property=\"schema:accessibilityFeature\">readingOrder</meta>\n\
    <meta property=\"schema:accessibilityHazard\">none</meta>\n\
    <meta property=\"schema:accessibilitySummary\">The text has a table of contents and a linear reading order. It has no images that carry meaning beyond the cover.</meta>\n",
    );
    // Second precision with a literal Z, which is what the spec asks for.
    let modified = m
        .frozen_at
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    meta.push_str(&format!(
        "    <meta property=\"dcterms:modified\">{modified}</meta>\n"
    ));

    let direction = if m.rtl {
        " page-progression-direction=\"rtl\""
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id" xml:lang="{lang}">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
{meta}  </metadata>
  <manifest>
{manifest}  </manifest>
  <spine{direction}>
{spine}  </spine>
</package>"#,
        lang = xml_escape(m.lang()),
    )
}

const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

/// Deliberately restrained: an e-reader's own typography is usually better than
/// anything imposed on it, and readers let people override this anyway.
const STYLESHEET: &str = r#"body { margin: 0 1em; line-height: 1.5; }
h1 { text-align: center; margin: 2em 0 1.5em; font-weight: normal; }
p { margin: 0; text-indent: 1.5em; text-align: justify; }
p.first, h1 + p, blockquote + p, hr + p { text-indent: 0; }
hr.scene { border: none; text-align: center; margin: 1.5em 0; }
hr.scene::after { content: "⁂"; }
blockquote { margin: 1.5em 2em; font-style: italic; }
.title-page { text-align: center; margin-top: 20%; }
.title-page h1 { font-size: 2em; }
.subtitle { text-indent: 0; font-style: italic; }
.byline { text-indent: 0; margin-top: 2em; }
.copyright, .dedication { text-indent: 0; margin-top: 3em; font-size: 0.9em; }
.epigraph { text-align: left; }
.epigraph .source { text-indent: 0; text-align: right; font-style: normal; }
.cover { text-align: center; }
.cover img { max-width: 100%; height: auto; }
"#;

const NOTES_STYLE: &str = r#"section.footnotes { margin-top: 2em; font-size: 0.9em; }
section.footnotes h2 { font-size: 1em; text-align: left; }
aside p { text-indent: 0; margin: 0.4em 0; }
sup a { text-decoration: none; }
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::options::{EpubOptions, Format};
    use crate::export::{Contributor, Cover, Note, SceneContent};
    use std::io::Read;

    fn book() -> Manuscript {
        let para = |t: &str| Block::Paragraph(vec![Span { text: t.into(), ..Default::default() }]);
        Manuscript {
            title: "Fish & Chips".into(),
            byline: "A. Writer".into(),
            chapters: (1..=3)
                .map(|n| ChapterContent {
                    id: format!("c{n}"),
                    title: format!("Chapter {n}"),
                    scenes: vec![SceneContent { blocks: vec![para("Text.")], ..Default::default() }],
                    ..Default::default()
                })
                .collect(),
            toc: true,
            ..Default::default()
        }
    }

    fn options() -> ExportOptions {
        ExportOptions {
            format: Format::Epub,
            ..Default::default()
        }
    }

    fn files(bytes: Vec<u8>) -> std::collections::BTreeMap<String, String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = std::collections::BTreeMap::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut body = Vec::new();
            f.read_to_end(&mut body).unwrap();
            out.insert(f.name().to_string(), String::from_utf8_lossy(&body).to_string());
        }
        out
    }

    #[test]
    fn mimetype_is_first_and_stored() {
        let bytes = write(&book(), &options()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let first = zip.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }

    #[test]
    fn chapters_are_split_into_files_by_default() {
        let f = files(write(&book(), &options()).unwrap());
        for n in 1..=3 {
            assert!(f.contains_key(&format!("OEBPS/chapter{n}.xhtml")));
        }
        assert!(f["OEBPS/chapter1.xhtml"].contains("<body epub:type=\"chapter\">"));
        assert!(f["OEBPS/nav.xhtml"].contains("href=\"chapter2.xhtml\""));
        assert!(f["OEBPS/content.opf"].contains("<itemref idref=\"chapter3\"/>"));
    }

    #[test]
    fn a_single_file_book_navigates_by_anchor() {
        let mut o = options();
        o.epub = Some(EpubOptions { split_chapters: false, cover_page: true });
        let f = files(write(&book(), &o).unwrap());
        assert!(!f.contains_key("OEBPS/chapter1.xhtml"));
        assert!(f["OEBPS/content.xhtml"].contains("<section epub:type=\"chapter\" id=\"chapter-2\">"));
        assert!(f["OEBPS/nav.xhtml"].contains("href=\"content.xhtml#chapter-2\""));
    }

    #[test]
    fn metadata_reaches_the_package() {
        let mut m = book();
        m.language = "he".into();
        m.rtl = true;
        m.description = Some("A <test>".into());
        m.publisher = Some("Press".into());
        m.published = Some("2026-03-01".into());
        m.identifier = Some("urn:isbn:9780000000002".into());
        m.keywords = vec!["fish".into()];
        m.contributors = vec![Contributor { name: "E. Ditor".into(), role: "editor".into() }];
        let f = files(write(&m, &options()).unwrap());
        let opf = &f["OEBPS/content.opf"];
        assert!(opf.contains("<dc:language>he</dc:language>"));
        assert!(opf.contains("<spine page-progression-direction=\"rtl\">"));
        assert!(opf.contains("<dc:description>A &lt;test&gt;</dc:description>"));
        assert!(opf.contains("urn:isbn:9780000000002"));
        assert!(opf.contains("property=\"role\" scheme=\"marc:relators\">edt<"));
        assert!(opf.contains("schema:accessMode"));
        assert!(f["OEBPS/chapter1.xhtml"].contains("dir=\"rtl\""));
    }

    #[test]
    fn a_cover_is_an_image_a_page_and_a_property() {
        let mut m = book();
        m.cover = Some(Cover { bytes: vec![0xff, 0xd8, 0xff], mime: "image/jpeg".into() });
        let f = files(write(&m, &options()).unwrap());
        let opf = &f["OEBPS/content.opf"];
        assert!(f.contains_key("OEBPS/cover.jpg"));
        assert!(opf.contains("properties=\"cover-image\""));
        assert!(opf.contains("<itemref idref=\"cover\"/>"));
        // The cover is the first thing in the spine.
        assert!(opf.find("idref=\"cover\"").unwrap() < opf.find("idref=\"title\"").unwrap());
    }

    #[test]
    fn notes_are_asides_with_noterefs() {
        let mut m = book();
        m.note_layout = NoteLayout::Footnotes;
        m.chapters[0].scenes[0].blocks = vec![Block::Paragraph(vec![
            Span { text: "Hi".into(), ..Default::default() },
            Span { note: Some(1), ..Default::default() },
        ])];
        m.chapters[0].notes = vec![Note { number: 1, text: "n".into(), quote: None }];
        let f = files(write(&m, &options()).unwrap());
        let page = &f["OEBPS/chapter1.xhtml"];
        assert!(page.contains("epub:type=\"noteref\" id=\"rc1-1\" href=\"#nc1-1\""));
        assert!(page.contains("<aside epub:type=\"footnote\" id=\"nc1-1\">"));
        assert!(page.contains("href=\"chapter1.xhtml#rc1-1\""));
    }

    #[test]
    fn book_endnotes_are_a_page_in_the_spine_and_the_nav() {
        let mut m = book();
        m.note_layout = NoteLayout::EndnotesBook;
        m.chapters[1].notes = vec![Note { number: 1, text: "n".into(), quote: Some("q".into()) }];
        let f = files(write(&m, &options()).unwrap());
        assert!(f.contains_key("OEBPS/notes.xhtml"));
        assert!(f["OEBPS/nav.xhtml"].contains("href=\"notes.xhtml\""));
        assert!(f["OEBPS/content.opf"].contains("idref=\"notes\""));
        assert!(f["OEBPS/notes.xhtml"].contains("href=\"chapter2.xhtml#rc2-1\""));
    }

    #[test]
    fn front_matter_can_be_left_out() {
        let mut m = book();
        m.omit_front_matter = true;
        let f = files(write(&m, &options()).unwrap());
        assert!(!f.contains_key("OEBPS/title.xhtml"));
        assert!(!f["OEBPS/content.opf"].contains("idref=\"title\""));
    }

    #[test]
    fn a_script_in_the_text_is_text() {
        let mut m = book();
        m.chapters[0].scenes[0].blocks = vec![Block::Paragraph(vec![Span {
            text: "<script>alert(1)</script>".into(),
            ..Default::default()
        }])];
        let f = files(write(&m, &options()).unwrap());
        assert!(!f["OEBPS/chapter1.xhtml"].contains("<script>"));
        assert!(f["OEBPS/chapter1.xhtml"].contains("&lt;script&gt;"));
    }

    #[test]
    fn the_self_check_catches_a_dangling_navigation_target() {
        let parts = vec![Part::page("OEBPS/a.xhtml", "a", String::new())];
        let nav = vec![NavEntry { href: "missing.xhtml".into(), title: "x".into() }];
        assert!(verify(&parts, &nav).unwrap_err().to_string().contains("missing.xhtml"));
        assert!(verify(&[], &[]).is_err());
    }
}
