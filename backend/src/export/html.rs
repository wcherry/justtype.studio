//! HTML output, in three shapes.
//!
//! *Styled* is one self-contained page: the whole book, styles inline, nothing
//! to fetch — for reading in a browser, or printing. *Clean* is semantic markup
//! and nothing else, no classes and no styles, to paste into a CMS or hand to a
//! converter. *Package* is a zip of pages, one per chapter, with a stylesheet,
//! the cover and links between them, that opens offline.
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::epub::spans_html;
use super::options::{ExportOptions, HtmlMode};
use super::web::{style_overrides, NoteMarkup};
use super::{xml_escape, Block, ChapterContent, Manuscript, NoteLayout, Role};

const STYLE: &str =
    "body{max-width:36em;margin:3em auto;padding:0 1em;font:1.1em/1.6 Georgia,serif;color:#222}\
h1,h2{font-weight:normal}.title-page{text-align:center;margin:6em 0}.title-page h1{font-size:2.4em}\
.byline,.subtitle{font-style:italic}.dedication,.epigraph{margin:2em auto;max-width:24em}\
section.chapter{margin-top:4em}p{margin:0;text-indent:1.5em}p.first{text-indent:0}\
hr.scene{border:0;text-align:center;margin:1.5em 0}hr.scene::after{content:\"* * *\"}\
blockquote{margin:1em 2em;font-style:italic}";

/// A part's title, set alone on its page; added only to a book with parts.
const PART_STYLE: &str = "h2.part-title{text-align:center;font-size:1.8em;margin:4em 0 2em}";

/// Footnotes and a stylesheet for printing them, added only when there are
/// notes, so a book without any is the file it always was.
const NOTES_STYLE: &str = "section.footnotes{margin-top:2em;border-top:1px solid #999;\
font-size:.9em}section.footnotes p{text-indent:0;margin:.4em 0}sup a{text-decoration:none}\
@media print{a{color:inherit;text-decoration:none}}";

const PACKAGE_STYLE: &str = "nav.pager{display:flex;justify-content:space-between;margin:2em 0}\
img.cover{display:block;max-width:100%;margin:2em auto}\
@media (max-width:40em){body{margin:1em auto}}";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flavor {
    Styled,
    Clean,
    Package,
}

impl Flavor {
    fn classed(self) -> bool {
        self != Flavor::Clean
    }
}

fn has_notes(m: &Manuscript) -> bool {
    m.chapters.iter().any(|c| !c.notes.is_empty())
}

fn chapter_anchor(index: usize) -> String {
    format!("chapter-{}", index + 1)
}

fn block_markup(
    blocks: &[Block],
    flavor: Flavor,
    notes: &NoteMarkup,
    first_heading_level: u8,
) -> String {
    let mut body = String::new();
    let mut first = true;
    let classed = flavor.classed();
    for block in blocks {
        match block {
            Block::Paragraph(spans) => {
                let class = if first && classed {
                    " class=\"first\""
                } else {
                    ""
                };
                first = false;
                body.push_str(&format!(
                    "<p{class}>{}</p>\n",
                    spans_html(spans, Some(notes))
                ));
            }
            Block::Heading { level, spans } => {
                first = true;
                let level = (*level).clamp(first_heading_level, 6);
                body.push_str(&format!(
                    "<h{level}>{}</h{level}>\n",
                    spans_html(spans, Some(notes))
                ));
            }
            Block::Quote(spans) => {
                first = true;
                body.push_str(&format!(
                    "<blockquote><p>{}</p></blockquote>\n",
                    spans_html(spans, Some(notes))
                ));
            }
            Block::SceneBreak => {
                first = true;
                body.push_str(scene_break(classed));
            }
        }
    }
    body
}

fn scene_break(classed: bool) -> &'static str {
    if classed {
        "<hr class=\"scene\">\n"
    } else {
        "<hr>\n"
    }
}

/// Whether the notes of this chapter belong on its own page.
fn notes_here(m: &Manuscript, chapter: &ChapterContent) -> bool {
    m.note_layout != NoteLayout::EndnotesBook && !chapter.notes.is_empty()
}

fn chapter_section(
    m: &Manuscript,
    index: usize,
    chapter: &ChapterContent,
    flavor: Flavor,
    notes_page: Option<&str>,
    with_id: bool,
) -> String {
    let markup = NoteMarkup {
        scope: format!("c{}", index + 1),
        notes_page: notes_page.map(str::to_string),
        epub: false,
        classed: flavor.classed(),
    };
    let mut classes = String::new();
    if flavor.classed() {
        classes.push_str("chapter");
        match chapter.role {
            Role::Front => classes.push_str(" front"),
            Role::Back => classes.push_str(" back"),
            Role::Part => classes.push_str(" part"),
            Role::Chapter => {}
        }
    }
    let class_attr = if classes.is_empty() {
        String::new()
    } else {
        format!(" class=\"{classes}\"")
    };
    let id_attr = if with_id {
        format!(" id=\"{}\"", chapter_anchor(index))
    } else {
        String::new()
    };
    let mut out = format!(
        "<section{class_attr}{id_attr}>\n<h2{}>{}</h2>\n",
        super::web::heading_class(chapter),
        xml_escape(&chapter.title)
    );
    for (scene_index, scene) in chapter.scenes.iter().enumerate() {
        if scene_index > 0 {
            out.push_str(scene_break(flavor.classed()));
        }
        out.push_str(&block_markup(&scene.blocks, flavor, &markup, 3));
    }
    if notes_here(m, chapter) {
        let scope = markup.scope.clone();
        let refs: Vec<(&str, &super::Note)> =
            chapter.notes.iter().map(|n| (scope.as_str(), n)).collect();
        out.push_str(&markup.section(&refs, m.note_label, |_| String::new()));
    }
    out.push_str("</section>\n");
    out
}

fn book_notes(m: &Manuscript, flavor: Flavor, page_of_chapter: impl Fn(usize) -> String) -> String {
    if m.note_layout != NoteLayout::EndnotesBook {
        return String::new();
    }
    let markup = NoteMarkup {
        scope: String::new(),
        notes_page: None,
        epub: false,
        classed: flavor.classed(),
    };
    let scopes: Vec<String> = (0..m.chapters.len())
        .map(|i| format!("c{}", i + 1))
        .collect();
    let all: Vec<(&str, &super::Note)> = m
        .chapters
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.notes.iter().map(move |n| (i, n)))
        .map(|(i, n)| (scopes[i].as_str(), n))
        .collect();
    markup.section(&all, m.note_label, |scope| {
        let index: usize = scope.trim_start_matches('c').parse::<usize>().unwrap_or(1) - 1;
        page_of_chapter(index)
    })
}

fn title_block(m: &Manuscript, flavor: Flavor) -> String {
    let classed = flavor.classed();
    let tag = |class: &str| {
        if classed {
            format!(" class=\"{class}\"")
        } else {
            String::new()
        }
    };
    let mut body = format!(
        "<header{}>\n<h1>{}</h1>\n",
        tag("title-page"),
        xml_escape(&m.title)
    );
    if let Some(subtitle) = &m.subtitle {
        body.push_str(&format!(
            "<p{}>{}</p>\n",
            tag("subtitle"),
            xml_escape(subtitle)
        ));
    }
    body.push_str(&format!(
        "<p{}>by {}</p>\n",
        tag("byline"),
        xml_escape(&m.byline)
    ));
    if let Some(dedication) = &m.dedication {
        body.push_str(&format!(
            "<p{}>{}</p>\n",
            tag("dedication"),
            xml_escape(dedication)
        ));
    }
    if let Some(epigraph) = &m.epigraph {
        body.push_str(&format!(
            "<blockquote{}><p>{}</p>",
            tag("epigraph"),
            xml_escape(epigraph)
        ));
        if let Some(source) = &m.epigraph_source {
            body.push_str(&format!("<p{}>— {}</p>", tag("source"), xml_escape(source)));
        }
        body.push_str("</blockquote>\n");
    }
    body.push_str("</header>\n");
    body
}

fn toc_block(m: &Manuscript, href: impl Fn(usize) -> String) -> String {
    let mut out = String::from("<nav>\n<h2>Contents</h2>\n<ol>\n");
    for (index, chapter) in m.chapters.iter().enumerate() {
        out.push_str(&format!(
            "<li><a href=\"{}\">{}</a></li>\n",
            href(index),
            xml_escape(&chapter.title)
        ));
    }
    out.push_str("</ol>\n</nav>\n");
    out
}

/// The attributes that say what language a page is in and which way it reads.
fn lang_attrs(m: &Manuscript) -> String {
    let mut attrs = format!("lang=\"{}\"", xml_escape(m.lang()));
    if m.rtl {
        attrs.push_str(" dir=\"rtl\"");
    }
    attrs
}

fn head_meta(m: &Manuscript) -> String {
    let mut meta = String::new();
    if m.has_details() {
        if let Some(description) = &m.description {
            meta.push_str(&format!(
                "<meta name=\"description\" content=\"{}\">\n",
                xml_escape(description)
            ));
        }
        meta.push_str(&format!(
            "<meta name=\"author\" content=\"{}\">\n",
            xml_escape(&m.byline)
        ));
    }
    meta
}

fn stylesheet(m: &Manuscript, options: &ExportOptions, extra: &str) -> String {
    let mut css = STYLE.to_string();
    if has_notes(m) {
        css.push_str(NOTES_STYLE);
    }
    if m.chapters.iter().any(|c| c.role == Role::Part) {
        css.push_str(PART_STYLE);
    }
    css.push_str(extra);
    let overrides = style_overrides(&options.style.resolve(), "h2", false);
    if !overrides.is_empty() {
        css.push('\n');
        css.push_str(&overrides);
    }
    css
}

pub fn write(manuscript: &Manuscript, options: &ExportOptions) -> String {
    let flavor = match options.html().mode {
        HtmlMode::Clean => Flavor::Clean,
        _ => Flavor::Styled,
    };
    let m = manuscript;

    let mut body = String::new();
    if !m.omit_front_matter {
        body.push_str(&title_block(m, flavor));
    }
    if m.toc {
        body.push_str(&toc_block(m, |i| format!("#{}", chapter_anchor(i))));
    }
    for (index, chapter) in m.chapters.iter().enumerate() {
        body.push_str(&chapter_section(m, index, chapter, flavor, None, m.toc));
    }
    body.push_str(&book_notes(m, flavor, |_| String::new()));
    // A book-wide notes section links back to marks that are on this page.

    if flavor == Flavor::Clean {
        return format!("<article {}>\n{body}</article>\n", lang_attrs(m));
    }

    format!(
        "<!DOCTYPE html>\n<html {lang}>\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n{meta}<title>{title}</title>\n\
<style>{css}</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        lang = lang_attrs(m),
        meta = head_meta(m),
        title = xml_escape(&m.title),
        css = stylesheet(m, options, ""),
    )
}

fn page(m: &Manuscript, title: &str, body: &str) -> String {
    format!(
        "<!DOCTYPE html>\n<html {lang}>\n<head>\n<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n{meta}<title>{title}</title>\n\
<link rel=\"stylesheet\" href=\"style.css\">\n</head>\n<body>\n{body}</body>\n</html>\n",
        lang = lang_attrs(m),
        meta = head_meta(m),
        title = xml_escape(title),
    )
}

fn chapter_file(index: usize) -> String {
    format!("chapter-{}.html", index + 1)
}

/// A zip of pages that opens offline: the front page with the contents, a page
/// per chapter with links to the ones either side, the stylesheet and cover.
pub fn write_package(manuscript: &Manuscript, options: &ExportOptions) -> std::io::Result<Vec<u8>> {
    let m = manuscript;
    let flavor = Flavor::Package;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut part = |name: &str, body: &[u8]| -> std::io::Result<()> {
        zip.start_file(name, deflated)?;
        zip.write_all(body)
    };

    part(
        "style.css",
        format!("{}{}", stylesheet(m, options, PACKAGE_STYLE), "\n").as_bytes(),
    )?;

    let mut front = String::new();
    if let Some(cover) = &m.cover {
        let name = format!("cover.{}", cover.extension());
        part(&name, &cover.bytes)?;
        front.push_str(&format!(
            "<img class=\"cover\" src=\"{name}\" alt=\"Cover of {}\">\n",
            xml_escape(&m.title)
        ));
    }
    if !m.omit_front_matter {
        front.push_str(&title_block(m, flavor));
    }
    // A package always has a way in to its chapters.
    front.push_str(&toc_block(m, chapter_file));
    part("index.html", page(m, &m.title, &front).as_bytes())?;

    let count = m.chapters.len();
    for (index, chapter) in m.chapters.iter().enumerate() {
        let mut pager = String::from("<nav class=\"pager\">");
        if index > 0 {
            pager.push_str(&format!(
                "<a href=\"{}\">← Previous</a>",
                chapter_file(index - 1)
            ));
        } else {
            pager.push_str("<span></span>");
        }
        pager.push_str("<a href=\"index.html\">Contents</a>");
        if index + 1 < count {
            pager.push_str(&format!(
                "<a href=\"{}\">Next →</a>",
                chapter_file(index + 1)
            ));
        } else if m.note_layout == NoteLayout::EndnotesBook && has_notes(m) {
            pager.push_str("<a href=\"notes.html\">Notes →</a>");
        } else {
            pager.push_str("<span></span>");
        }
        pager.push_str("</nav>\n");

        let notes_page = (m.note_layout == NoteLayout::EndnotesBook).then_some("notes.html");
        let mut body = pager.clone();
        body.push_str(&chapter_section(
            m, index, chapter, flavor, notes_page, false,
        ));
        body.push_str(&pager);
        part(
            &chapter_file(index),
            page(m, &chapter.title, &body).as_bytes(),
        )?;
    }

    if m.note_layout == NoteLayout::EndnotesBook && has_notes(m) {
        let body = book_notes(m, flavor, chapter_file);
        part("notes.html", page(m, "Notes", &body).as_bytes())?;
    }

    Ok(zip.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::options::HtmlOptions;
    use crate::export::{ChapterContent, Cover, Note, SceneContent, Span};

    fn manuscript(text: &str) -> Manuscript {
        Manuscript {
            title: "Fish & Chips".into(),
            byline: "A".into(),
            surname: "A".into(),
            short_title: "F".into(),
            word_count: 2,
            chapters: vec![ChapterContent {
                title: "One".into(),
                scenes: vec![SceneContent {
                    blocks: vec![Block::Paragraph(vec![Span {
                        text: text.into(),
                        ..Span::default()
                    }])],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn with_mode(mode: HtmlMode) -> ExportOptions {
        ExportOptions {
            format: crate::export::options::Format::Html,
            html: Some(HtmlOptions { mode }),
            ..Default::default()
        }
    }

    #[test]
    fn a_book_comes_out_as_one_escaped_page() {
        let html = write(&manuscript("<hi>"), &ExportOptions::default());
        assert!(html.contains("<title>Fish &amp; Chips</title>"));
        assert!(html.contains("&lt;hi&gt;"));
        assert!(html.contains("<h2>One</h2>"));
    }

    #[test]
    fn a_script_in_a_scene_is_text_in_every_mode() {
        let evil = "<script>alert(1)</script>";
        for mode in [HtmlMode::Styled, HtmlMode::Clean] {
            let html = write(&manuscript(evil), &with_mode(mode));
            assert!(!html.contains("<script>"), "{html}");
            assert!(html.contains("&lt;script&gt;"));
        }
        let zip = write_package(&manuscript(evil), &with_mode(HtmlMode::Package)).unwrap();
        let text = String::from_utf8_lossy(&zip).to_string();
        // Deflated, so look inside rather than at the raw bytes.
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
        let mut page = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("chapter-1.html").unwrap(), &mut page)
            .unwrap();
        assert!(!page.contains("<script>"));
        assert!(page.contains("&lt;script&gt;"));
        let _ = text;
    }

    #[test]
    fn clean_html_has_no_classes_or_styles() {
        let mut m = manuscript("Hello.");
        m.chapters[0].scenes.push(SceneContent {
            blocks: vec![Block::Paragraph(vec![Span {
                text: "More.".into(),
                ..Span::default()
            }])],
            ..Default::default()
        });
        m.dedication = Some("For you".into());
        m.epigraph = Some("Words".into());
        m.epigraph_source = Some("Someone".into());
        m.toc = true;
        m.chapters[0].role = Role::Back;
        let html = write(&m, &with_mode(HtmlMode::Clean));
        assert!(!html.contains("class="), "{html}");
        assert!(!html.contains("style"), "{html}");
        assert!(!html.contains("<style"), "{html}");
        assert!(html.starts_with("<article lang=\"en\">"));
        assert!(html.contains("<hr>"));
        assert!(html.contains("href=\"#chapter-1\""));
    }

    #[test]
    fn language_and_direction_are_set() {
        let mut m = manuscript("x");
        m.language = "he".into();
        m.rtl = true;
        m.description = Some("About \"it\"".into());
        let html = write(&m, &ExportOptions::default());
        assert!(html.contains("<html lang=\"he\" dir=\"rtl\">"));
        assert!(html.contains("<meta name=\"description\" content=\"About &quot;it&quot;\">"));
        assert!(html.contains("<meta name=\"author\" content=\"A\">"));
    }

    #[test]
    fn footnotes_link_both_ways() {
        let mut m = manuscript("Hi");
        m.note_layout = NoteLayout::Footnotes;
        m.chapters[0].scenes[0].blocks = vec![Block::Paragraph(vec![
            Span {
                text: "Hi".into(),
                ..Span::default()
            },
            Span {
                note: Some(1),
                ..Span::default()
            },
        ])];
        m.chapters[0].notes = vec![Note {
            number: 1,
            text: "a & b".into(),
            quote: None,
        }];
        let html = write(&m, &ExportOptions::default());
        assert!(html.contains("<sup><a id=\"fnrefc1-1\" href=\"#fnc1-1\">1</a></sup>"));
        assert!(html
            .contains("<p id=\"fnc1-1\"><sup>1</sup> a &amp; b <a href=\"#fnrefc1-1\">↩</a></p>"));
        assert!(html.contains("section.footnotes"));
    }

    #[test]
    fn the_toc_links_to_chapters_by_anchor() {
        let mut m = manuscript("x");
        m.toc = true;
        let html = write(&m, &ExportOptions::default());
        assert!(html.contains("<a href=\"#chapter-1\">One</a>"));
        assert!(html.contains("<section class=\"chapter\" id=\"chapter-1\">"));
    }

    #[test]
    fn a_package_has_pages_styles_cover_and_links() {
        let mut m = manuscript("x");
        m.chapters.push(m.chapters[0].clone());
        m.cover = Some(Cover {
            bytes: vec![1, 2, 3],
            mime: "image/png".into(),
        });
        let bytes = write_package(&m, &with_mode(HtmlMode::Package)).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = archive.file_names().map(String::from).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "chapter-1.html",
                "chapter-2.html",
                "cover.png",
                "index.html",
                "style.css"
            ]
        );
        let mut first = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("chapter-1.html").unwrap(), &mut first)
            .unwrap();
        assert!(first.contains("href=\"chapter-2.html\""));
        assert!(first.contains("href=\"index.html\""));
        assert!(first.contains("href=\"style.css\""));
        let mut index = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("index.html").unwrap(), &mut index)
            .unwrap();
        assert!(index.contains("src=\"cover.png\""));
        assert!(index.contains("href=\"chapter-1.html\""));
    }

    #[test]
    fn book_endnotes_get_their_own_page_in_a_package() {
        let mut m = manuscript("x");
        m.note_layout = NoteLayout::EndnotesBook;
        m.chapters[0].notes = vec![Note {
            number: 1,
            text: "n".into(),
            quote: Some("x".into()),
        }];
        let bytes = write_package(&m, &with_mode(HtmlMode::Package)).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut notes = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("notes.html").unwrap(), &mut notes)
            .unwrap();
        assert!(notes.contains("href=\"chapter-1.html#fnrefc1-1\""));
    }

    #[test]
    fn typography_overrides_reach_the_stylesheet() {
        use crate::export::options::{StyleOptions, StylePreset};
        let options = ExportOptions {
            style: StyleOptions {
                preset: StylePreset::Book,
                font: Some("garamond".into()),
                scene_break: Some("* * *".into()),
                drop_caps: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let html = write(&manuscript("x"), &options);
        assert!(html.contains("font-family:Garamond"));
        assert!(html.contains("hr.scene::after{content:\"* * *\";}"));
        assert!(html.contains("::first-letter"));
    }
}
