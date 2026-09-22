//! DOCX output.
//!
//! A .docx is a ZIP of XML parts, written here directly rather than through a
//! library: the document is a flat run of paragraphs with four styles, and the
//! parts it needs are few enough that a dependency would be more surface to
//! audit than code to write.
//!
//! The manuscript style follows Shunn: Courier 12pt, double spaced, half-inch
//! first-line indents, `#` on its own line for a scene break, a contact block
//! and rounded word count on the first page, and a `Surname / TITLE / page`
//! header on every page after it.
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::{xml_escape, Block, Manuscript, Span, Style};

/// Twips — twentieths of a point — are the unit Word measures in.
const INCH: i32 = 1440;

pub fn write(manuscript: &Manuscript, style: Style) -> std::io::Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut part = |name: &str, body: &str| -> std::io::Result<()> {
        zip.start_file(name, options)?;
        zip.write_all(body.as_bytes())
    };

    part("[Content_Types].xml", CONTENT_TYPES)?;
    part("_rels/.rels", ROOT_RELS)?;
    part("word/_rels/document.xml.rels", DOCUMENT_RELS)?;
    part("word/styles.xml", &styles(style))?;
    part("word/header1.xml", &header(manuscript, style))?;
    part("word/document.xml", &document(manuscript, style))?;

    Ok(zip.finish()?.into_inner())
}

fn font(style: Style) -> &'static str {
    match style {
        // Courier is not nostalgia: a monospaced face makes a page's word count
        // predictable, which is the whole point of the format.
        Style::Manuscript => "Courier New",
        Style::Book => "Georgia",
    }
}

/// 480 twips = 24pt = double spacing at 12pt.
fn line_spacing(style: Style) -> i32 {
    match style {
        Style::Manuscript => 480,
        Style::Book => 276, // 1.15, which reads better on screen than single
    }
}

fn styles(style: Style) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault><w:rPr>
      <w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}"/>
      <w:sz w:val="24"/><w:szCs w:val="24"/>
    </w:rPr></w:rPrDefault>
    <w:pPrDefault><w:pPr>
      <w:spacing w:after="0" w:line="{line}" w:lineRule="auto"/>
    </w:pPr></w:pPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/>
  </w:style>
</w:styles>"#,
        font = font(style),
        line = line_spacing(style),
    )
}

/// `Surname / SHORT TITLE / page`, which is what lets an editor put a dropped
/// page back where it belongs.
fn header(manuscript: &Manuscript, style: Style) -> String {
    if style != Style::Manuscript {
        // A book has no running header of its own; the part still has to exist
        // because the section references it.
        return r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#
            .to_string();
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:p>
    <w:pPr><w:jc w:val="right"/><w:spacing w:line="240" w:lineRule="auto"/></w:pPr>
    <w:r><w:t xml:space="preserve">{surname} / {title} / </w:t></w:r>
    <w:fldSimple w:instr=" PAGE "><w:r><w:t>2</w:t></w:r></w:fldSimple>
  </w:p>
</w:hdr>"#,
        surname = xml_escape(&manuscript.surname),
        title = xml_escape(&manuscript.short_title.to_uppercase()),
    )
}

fn runs(spans: &[Span], style: Style) -> String {
    spans
        .iter()
        .map(|span| {
            let mut properties = String::new();
            if span.bold {
                properties.push_str("<w:b/>");
            }
            // Manuscript format wants underline where a book wants italic: it
            // survives a photocopier and a red pen, which is what it is for.
            if span.italic {
                properties.push_str(match style {
                    Style::Manuscript => r#"<w:u w:val="single"/>"#,
                    Style::Book => "<w:i/>",
                });
            }
            if span.underline {
                properties.push_str(r#"<w:u w:val="single"/>"#);
            }
            let properties = if properties.is_empty() {
                String::new()
            } else {
                format!("<w:rPr>{properties}</w:rPr>")
            };
            // A hard break inside a run is its own element, not a newline.
            let body = span
                .text
                .split('\n')
                .map(|line| format!(r#"<w:t xml:space="preserve">{}</w:t>"#, xml_escape(line)))
                .collect::<Vec<_>>()
                .join("<w:br/>");
            format!("<w:r>{properties}{body}</w:r>")
        })
        .collect()
}

fn paragraph(properties: &str, body: &str) -> String {
    format!("<w:p><w:pPr>{properties}</w:pPr>{body}</w:p>")
}

fn centred(text: &str, style: Style) -> String {
    paragraph(
        r#"<w:jc w:val="center"/>"#,
        &runs(
            &[Span {
                text: text.to_string(),
                ..Default::default()
            }],
            style,
        ),
    )
}

fn blank() -> String {
    "<w:p/>".to_string()
}

fn page_break() -> String {
    r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#.to_string()
}

fn front_matter(manuscript: &Manuscript, style: Style) -> String {
    let mut out = String::new();
    let single = r#"<w:spacing w:line="240" w:lineRule="auto"/>"#;

    if style == Style::Manuscript {
        // Contact block top left, word count top right, title a third of the
        // way down — the layout an editor's eye expects to skim.
        let contact = &manuscript.contact;
        for line in [
            contact
                .legal_name
                .clone()
                .or_else(|| Some(manuscript.byline.clone())),
            contact.address.clone(),
            contact.phone.clone(),
            contact.email.clone(),
        ]
        .into_iter()
        .flatten()
        {
            for part in line.split('\n') {
                out.push_str(&paragraph(
                    single,
                    &runs(
                        &[Span {
                            text: part.into(),
                            ..Default::default()
                        }],
                        style,
                    ),
                ));
            }
        }
        out.push_str(&paragraph(
            &format!(r#"<w:jc w:val="right"/>{single}"#),
            &runs(
                &[Span {
                    text: format!("about {} words", manuscript.rounded_word_count()),
                    ..Default::default()
                }],
                style,
            ),
        ));
        for _ in 0..8 {
            out.push_str(&blank());
        }
    }

    out.push_str(&centred(&manuscript.title, style));
    if let Some(subtitle) = &manuscript.subtitle {
        out.push_str(&centred(subtitle, style));
    }
    out.push_str(&blank());
    out.push_str(&centred(&format!("by {}", manuscript.byline), style));

    if style == Style::Book {
        if let Some(line) = &manuscript.copyright_line {
            out.push_str(&page_break());
            out.push_str(&centred(line, style));
        }
        if let Some(dedication) = &manuscript.dedication {
            out.push_str(&page_break());
            for _ in 0..6 {
                out.push_str(&blank());
            }
            out.push_str(&centred(dedication, style));
        }
        if let Some(epigraph) = &manuscript.epigraph {
            out.push_str(&page_break());
            for _ in 0..6 {
                out.push_str(&blank());
            }
            out.push_str(&centred(epigraph, style));
            if let Some(source) = &manuscript.epigraph_source {
                out.push_str(&centred(&format!("— {source}"), style));
            }
        }
    }
    out
}

fn body(manuscript: &Manuscript, style: Style) -> String {
    let indent = format!(r#"<w:ind w:firstLine="{}"/>"#, INCH / 2);
    let mut out = String::new();

    for (index, chapter) in manuscript.chapters.iter().enumerate() {
        // Every chapter starts on a fresh page, in both styles.
        out.push_str(&page_break());
        if style == Style::Book {
            for _ in 0..4 {
                out.push_str(&blank());
            }
        }
        out.push_str(&centred(&chapter.title, style));
        out.push_str(&blank());

        for (scene_index, scene) in chapter.scenes.iter().enumerate() {
            // A scene after the first in a chapter is separated, not titled:
            // scene titles are the writer's scaffolding, not the reader's.
            if scene_index > 0 {
                out.push_str(&centred(scene_break_glyph(style), style));
            }
            for block in &scene.blocks {
                out.push_str(&render_block(block, style, &indent));
            }
        }
        let _ = index;
    }
    out
}

fn scene_break_glyph(style: Style) -> &'static str {
    match style {
        // `#` is unambiguous on a printed page and survives a fax, which is
        // more than can be said for a decorative glyph.
        Style::Manuscript => "#",
        Style::Book => "⁂",
    }
}

fn render_block(block: &Block, style: Style, indent: &str) -> String {
    match block {
        Block::Paragraph(spans) => paragraph(indent, &runs(spans, style)),
        Block::Heading { spans, .. } => paragraph(r#"<w:jc w:val="center"/>"#, &runs(spans, style)),
        Block::Quote(spans) => paragraph(
            &format!(r#"<w:ind w:left="{}" w:right="{}"/>"#, INCH / 2, INCH / 2),
            &runs(spans, style),
        ),
        Block::SceneBreak => centred(scene_break_glyph(style), style),
    }
}

fn document(manuscript: &Manuscript, style: Style) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    {front}
    {body}
    <w:sectPr>
      <w:headerReference w:type="default" r:id="rId2"/>
      <w:titlePg/>
      <w:pgSz w:w="12240" w:h="15840"/>
      <w:pgMar w:top="{margin}" w:right="{margin}" w:bottom="{margin}" w:left="{margin}"
               w:header="{header}" w:footer="{header}" w:gutter="0"/>
    </w:sectPr>
  </w:body>
</w:document>"#,
        front = front_matter(manuscript, style),
        body = body(manuscript, style),
        margin = INCH,
        header = INCH / 2,
    )
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
</Relationships>"#;
