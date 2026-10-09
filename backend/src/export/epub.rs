//! EPUB 3 output, for beta readers who would rather read on an e-reader than in
//! a browser.
//!
//! The container is a ZIP with one rule that matters: `mimetype` must be the
//! first entry and stored uncompressed, or readers reject the file before
//! opening anything else.
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::options::ExportOptions;
use super::{xml_escape, Block, ChapterContent, Manuscript, Span};

pub fn write(manuscript: &Manuscript, _options: &ExportOptions) -> std::io::Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // First and uncompressed, per the specification.
    zip.start_file("mimetype", stored)?;
    zip.write_all(b"application/epub+zip")?;

    let mut part = |name: &str, body: &str| -> std::io::Result<()> {
        zip.start_file(name, deflated)?;
        zip.write_all(body.as_bytes())
    };

    part("META-INF/container.xml", CONTAINER)?;
    part("OEBPS/style.css", STYLESHEET)?;
    part("OEBPS/title.xhtml", &title_page(manuscript))?;

    for (index, chapter) in manuscript.chapters.iter().enumerate() {
        part(
            &format!("OEBPS/chapter{}.xhtml", index + 1),
            &chapter_page(chapter),
        )?;
    }

    part("OEBPS/nav.xhtml", &navigation(manuscript))?;
    part("OEBPS/content.opf", &package(manuscript))?;

    Ok(zip.finish()?.into_inner())
}

pub(super) fn spans_to_html(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| {
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

fn chapter_page(chapter: &ChapterContent) -> String {
    let mut body = format!("<h1>{}</h1>\n", xml_escape(&chapter.title));
    for (index, scene) in chapter.scenes.iter().enumerate() {
        if index > 0 {
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
                    format!("<p{class}>{}</p>", spans_to_html(spans))
                }
                Block::Heading { level, spans } => {
                    first = true;
                    let level = (*level).clamp(2, 6);
                    format!("<h{level}>{}</h{level}>", spans_to_html(spans))
                }
                Block::Quote(spans) => {
                    first = true;
                    format!("<blockquote><p>{}</p></blockquote>", spans_to_html(spans))
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
    page(&chapter.title, &body)
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
    page(&manuscript.title, &body)
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>{}</title><link rel="stylesheet" type="text/css" href="style.css"/></head>
<body>
{body}
</body>
</html>"#,
        xml_escape(title)
    )
}

fn navigation(manuscript: &Manuscript) -> String {
    let items: String = manuscript
        .chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| {
            format!(
                "      <li><a href=\"chapter{}.xhtml\">{}</a></li>\n",
                index + 1,
                xml_escape(&chapter.title)
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head><title>Contents</title></head>
<body>
  <nav epub:type="toc" id="toc">
    <h1>Contents</h1>
    <ol>
{items}    </ol>
  </nav>
</body>
</html>"#
    )
}

fn package(manuscript: &Manuscript) -> String {
    let manifest: String = manuscript
        .chapters
        .iter()
        .enumerate()
        .map(|(index, _)| {
            format!(
                "    <item id=\"chapter{n}\" href=\"chapter{n}.xhtml\" media-type=\"application/xhtml+xml\"/>\n",
                n = index + 1
            )
        })
        .collect();
    let spine: String = manuscript
        .chapters
        .iter()
        .enumerate()
        .map(|(index, _)| format!("    <itemref idref=\"chapter{}\"/>\n", index + 1))
        .collect();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:uuid:{id}</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:creator>{author}</dc:creator>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">{modified}</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="css" href="style.css" media-type="text/css"/>
    <item id="title" href="title.xhtml" media-type="application/xhtml+xml"/>
{manifest}  </manifest>
  <spine>
    <itemref idref="title"/>
{spine}  </spine>
</package>"#,
        id = uuid::Uuid::new_v4(),
        title = xml_escape(&manuscript.title),
        author = xml_escape(&manuscript.byline),
        // Second precision with a literal Z, which is what the spec asks for.
        modified = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
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
"#;
