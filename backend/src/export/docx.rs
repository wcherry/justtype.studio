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
use std::collections::HashMap;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::options::{Align, ExportOptions, HeadingStyle, Numbering, ResolvedStyle};
use super::{xml_escape, Block, Manuscript, Note, NoteLayout, Span, Style};

/// Twips — twentieths of a point — are the unit Word measures in.
const INCH: i32 = 1440;

/// What the writer needs to know beyond the manuscript itself.
struct Ctx<'a> {
    m: &'a Manuscript,
    style: Style,
    resolved: ResolvedStyle,
    /// Word numbers notes itself; these are the ids tying a reference to its
    /// note, unique across the book, in reading order.
    note_ids: HashMap<(usize, u32), u32>,
    /// Chapters are separate sections, so notes can restart or collect at the
    /// end of each.
    sectioned: bool,
    restart_notes: bool,
}

impl Ctx<'_> {
    fn endnotes(&self) -> bool {
        matches!(
            self.m.note_layout,
            NoteLayout::EndnotesChapter | NoteLayout::EndnotesBook
        )
    }
    fn has_notes(&self) -> bool {
        self.m.note_layout != NoteLayout::None && !self.note_ids.is_empty()
    }
}

pub fn write(manuscript: &Manuscript, options: &ExportOptions) -> std::io::Result<Vec<u8>> {
    let style = options.style_kind();
    let mut note_ids = HashMap::new();
    let mut next = 1;
    if manuscript.note_layout != NoteLayout::None {
        for (ci, chapter) in manuscript.chapters.iter().enumerate() {
            for note in &chapter.notes {
                note_ids.insert((ci, note.number), next);
                next += 1;
            }
        }
    }
    let per_chapter = options.content.comment_numbering == Numbering::Chapter;
    let layout = manuscript.note_layout;
    let ctx = Ctx {
        m: manuscript,
        style,
        resolved: options.style.resolve(),
        sectioned: !note_ids.is_empty()
            && manuscript.chapters.len() > 1
            && (layout == NoteLayout::EndnotesChapter
                || (per_chapter && layout != NoteLayout::None)),
        restart_notes: per_chapter,
        note_ids,
    };
    write_with(&ctx)
}

fn write_with(ctx: &Ctx) -> std::io::Result<Vec<u8>> {
    let hyphenate = ctx.resolved.hyphenate;
    let manuscript = ctx.m;
    let style = ctx.style;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut part = |name: &str, body: &str| -> std::io::Result<()> {
        zip.start_file(name, options)?;
        zip.write_all(body.as_bytes())
    };

    let details = manuscript.has_details();
    let notes = ctx.has_notes();
    let settings = notes || hyphenate;

    part("[Content_Types].xml", &content_types(ctx, details, settings))?;
    part("_rels/.rels", &root_rels(details))?;
    part("word/_rels/document.xml.rels", &document_rels(ctx, settings))?;
    part("word/styles.xml", &styles(ctx))?;
    part("word/header1.xml", &header(manuscript, style))?;
    part("word/document.xml", &document(ctx))?;
    if settings {
        part("word/settings.xml", &settings_xml(ctx, hyphenate))?;
    }
    if notes && !ctx.endnotes() {
        part("word/footnotes.xml", &notes_part(ctx, false))?;
    }
    if notes && ctx.endnotes() {
        part("word/endnotes.xml", &notes_part(ctx, true))?;
    }
    if details {
        part("docProps/core.xml", &core_properties(manuscript))?;
    }

    Ok(zip.finish()?.into_inner())
}

fn font(ctx: &Ctx) -> String {
    if ctx.resolved.customised {
        return ctx.resolved.font.clone();
    }
    match ctx.style {
        // Courier is not nostalgia: a monospaced face makes a page's word count
        // predictable, which is the whole point of the format.
        Style::Manuscript => "Courier New".to_string(),
        Style::Book => "Georgia".to_string(),
    }
}

/// 480 twips = 24pt = double spacing at 12pt.
fn line_spacing(ctx: &Ctx) -> i32 {
    if ctx.resolved.customised {
        return ctx.resolved.line_spacing.twips();
    }
    match ctx.style {
        Style::Manuscript => 480,
        Style::Book => 276, // 1.15, which reads better on screen than single
    }
}

/// Half-points, which is what `w:sz` takes.
fn half_points(ctx: &Ctx) -> i32 {
    if ctx.resolved.customised {
        (ctx.resolved.size_pt * 2.0).round() as i32
    } else {
        24
    }
}

fn styles(ctx: &Ctx) -> String {
    let notes = if ctx.has_notes() {
        r#"
  <w:style w:type="character" w:styleId="FootnoteReference">
    <w:name w:val="footnote reference"/>
    <w:rPr><w:vertAlign w:val="superscript"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="FootnoteText">
    <w:name w:val="footnote text"/>
    <w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>
    <w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>
  </w:style>
  <w:style w:type="paragraph" w:styleId="EndnoteText">
    <w:name w:val="endnote text"/>
    <w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>
    <w:rPr><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>
  </w:style>"#
    } else {
        ""
    };
    let lang = if ctx.m.has_details() {
        format!(r#"<w:lang w:val="{}"/>"#, xml_escape(ctx.m.lang()))
    } else {
        String::new()
    };
    let bidi = if ctx.m.rtl { "<w:bidi/>" } else { "" };
    let size = half_points(ctx);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault><w:rPr>
      <w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}"/>
      <w:sz w:val="{size}"/><w:szCs w:val="{size}"/>{lang}
    </w:rPr></w:rPrDefault>
    <w:pPrDefault><w:pPr>{bidi}
      <w:spacing w:after="0" w:line="{line}" w:lineRule="auto"/>
    </w:pPr></w:pPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/>
  </w:style>{notes}
</w:styles>"#,
        font = xml_escape(&font(ctx)),
        line = line_spacing(ctx),
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

/// Runs for a stretch of text. `chapter` is the index of the chapter the spans
/// are in, so a note reference can find its id.
fn runs(ctx: &Ctx, spans: &[Span], chapter: usize) -> String {
    spans
        .iter()
        .map(|span| {
            if let Some(number) = span.note {
                let Some(id) = ctx.note_ids.get(&(chapter, number)) else {
                    return String::new();
                };
                let (style, element) = if ctx.endnotes() {
                    ("EndnoteReference", "endnoteReference")
                } else {
                    ("FootnoteReference", "footnoteReference")
                };
                let _ = style;
                return format!(
                    r#"<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:{element} w:id="{id}"/></w:r>"#
                );
            }
            let mut properties = String::new();
            if span.bold {
                properties.push_str("<w:b/>");
            }
            // Manuscript format wants underline where a book wants italic: it
            // survives a photocopier and a red pen, which is what it is for.
            if span.italic {
                properties.push_str(match ctx.style {
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

fn plain(text: &str) -> Vec<Span> {
    vec![Span {
        text: text.to_string(),
        ..Default::default()
    }]
}

fn paragraph(properties: &str, body: &str) -> String {
    format!("<w:p><w:pPr>{properties}</w:pPr>{body}</w:p>")
}

fn centred(ctx: &Ctx, text: &str) -> String {
    paragraph(
        r#"<w:jc w:val="center"/>"#,
        &runs(ctx, &plain(text), 0),
    )
}

fn blank() -> String {
    "<w:p/>".to_string()
}

fn page_break() -> String {
    r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#.to_string()
}

fn front_matter(ctx: &Ctx) -> String {
    let manuscript = ctx.m;
    let style = ctx.style;
    let mut out = String::new();
    let single = r#"<w:spacing w:line="240" w:lineRule="auto"/>"#;

    if manuscript.omit_front_matter {
        return out;
    }

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
                out.push_str(&paragraph(single, &runs(ctx, &plain(part), 0)));
            }
        }
        out.push_str(&paragraph(
            &format!(r#"<w:jc w:val="right"/>{single}"#),
            &runs(
                ctx,
                &plain(&format!("about {} words", manuscript.rounded_word_count())),
                0,
            ),
        ));
        for _ in 0..8 {
            out.push_str(&blank());
        }
    }

    out.push_str(&centred(ctx, &manuscript.title));
    if let Some(subtitle) = &manuscript.subtitle {
        out.push_str(&centred(ctx, subtitle));
    }
    out.push_str(&blank());
    out.push_str(&centred(ctx, &format!("by {}", manuscript.byline)));

    if style == Style::Book {
        if let Some(line) = &manuscript.copyright_line {
            out.push_str(&page_break());
            out.push_str(&centred(ctx, line));
        }
        if let Some(dedication) = &manuscript.dedication {
            out.push_str(&page_break());
            for _ in 0..6 {
                out.push_str(&blank());
            }
            out.push_str(&centred(ctx, dedication));
        }
        if let Some(epigraph) = &manuscript.epigraph {
            out.push_str(&page_break());
            for _ in 0..6 {
                out.push_str(&blank());
            }
            out.push_str(&centred(ctx, epigraph));
            if let Some(source) = &manuscript.epigraph_source {
                out.push_str(&centred(ctx, &format!("— {source}")));
            }
        }
    }
    out
}

/// A contents page: chapter titles that jump to their chapters. No page
/// numbers — Word cannot know them until it lays the pages out, and a field
/// that wants updating on open is a prompt the writer did not ask for.
fn contents(ctx: &Ctx) -> String {
    if !ctx.m.toc {
        return String::new();
    }
    let mut out = page_break();
    out.push_str(&centred(ctx, "Contents"));
    out.push_str(&blank());
    for (index, chapter) in ctx.m.chapters.iter().enumerate() {
        out.push_str(&paragraph(
            r#"<w:ind w:left="720"/>"#,
            &format!(
                r#"<w:hyperlink w:anchor="chapter{}" w:history="1">{}</w:hyperlink>"#,
                index + 1,
                runs(ctx, &plain(&chapter.title), 0)
            ),
        ));
    }
    out
}

fn sect_props(ctx: &Ctx, first: bool) -> String {
    let mut props = String::new();
    if first {
        props.push_str(r#"<w:headerReference w:type="default" r:id="rId2"/>"#);
    }
    if ctx.restart_notes && !ctx.endnotes() {
        props.push_str(r#"<w:footnotePr><w:numRestart w:val="eachSect"/></w:footnotePr>"#);
    }
    if ctx.endnotes() && ctx.m.note_layout == NoteLayout::EndnotesChapter {
        props.push_str(&format!(
            r#"<w:endnotePr><w:pos w:val="sectEnd"/>{}</w:endnotePr>"#,
            if ctx.restart_notes {
                r#"<w:numRestart w:val="eachSect"/>"#
            } else {
                ""
            }
        ));
    }
    props
}

fn page_setup() -> String {
    format!(
        r#"<w:pgSz w:w="12240" w:h="15840"/>
      <w:pgMar w:top="{margin}" w:right="{margin}" w:bottom="{margin}" w:left="{margin}"
               w:header="{header}" w:footer="{header}" w:gutter="0"/>"#,
        margin = INCH,
        header = INCH / 2,
    )
}

/// Ends a chapter's section, so its notes can restart or collect there.
fn section_break(ctx: &Ctx, first: bool) -> String {
    format!(
        r#"<w:p><w:pPr><w:sectPr>{props}<w:type w:val="nextPage"/>{setup}{title}</w:sectPr></w:pPr></w:p>"#,
        props = sect_props(ctx, first),
        setup = page_setup(),
        title = if first { "<w:titlePg/>" } else { "" },
    )
}

fn body(ctx: &Ctx) -> String {
    let manuscript = ctx.m;
    let style = ctx.style;
    let r = &ctx.resolved;
    let indent = if r.customised {
        let twips = (r.paragraph_indent_em * r.size_pt * 20.0).round() as i32;
        let after = (r.paragraph_spacing_em * r.size_pt * 20.0).round() as i32;
        format!(
            r#"<w:spacing w:after="{after}" w:line="{}" w:lineRule="auto"/><w:ind w:firstLine="{twips}"/><w:jc w:val="{}"/>"#,
            r.line_spacing.twips(),
            if r.align == Align::Justify { "both" } else { "left" }
        )
    } else {
        format!(r#"<w:ind w:firstLine="{}"/>"#, INCH / 2)
    };
    let mut out = String::new();

    for (index, chapter) in manuscript.chapters.iter().enumerate() {
        // Every chapter starts on a fresh page, in both styles.
        if ctx.sectioned && index > 0 {
            out.push_str(&section_break(ctx, index == 1));
        } else {
            out.push_str(&page_break());
        }
        if style == Style::Book {
            for _ in 0..4 {
                out.push_str(&blank());
            }
        }
        let heading = {
            let mut props = String::from(r#"<w:jc w:val="center"/>"#);
            let _ = &mut props;
            let runs_xml = if r.customised && r.heading_style == HeadingStyle::SmallCaps {
                runs(ctx, &plain(&chapter.title), index).replace("<w:r>", "<w:r><w:rPr><w:smallCaps/></w:rPr>")
            } else {
                runs(ctx, &plain(&chapter.title), index)
            };
            let runs_xml = if r.customised && r.heading_style == HeadingStyle::SmallCaps {
                runs_xml.replace("<w:rPr><w:smallCaps/></w:rPr><w:rPr>", "<w:rPr><w:smallCaps/>")
            } else {
                runs_xml
            };
            if manuscript.toc {
                format!(
                    r#"<w:bookmarkStart w:id="{id}" w:name="chapter{id}"/>{runs_xml}<w:bookmarkEnd w:id="{id}"/>"#,
                    id = index + 1
                )
            } else {
                runs_xml
            }
        };
        out.push_str(&paragraph(r#"<w:jc w:val="center"/>"#, &heading));
        out.push_str(&blank());

        for (scene_index, scene) in chapter.scenes.iter().enumerate() {
            // A scene after the first in a chapter is separated, not titled:
            // scene titles are the writer's scaffolding, not the reader's.
            if scene_index > 0 {
                out.push_str(&scene_break_para(ctx));
            }
            for block in &scene.blocks {
                out.push_str(&render_block(ctx, block, &indent, index));
            }
        }
    }
    out
}

fn scene_break_para(ctx: &Ctx) -> String {
    if ctx.resolved.scene_break.is_empty() {
        return blank();
    }
    centred(ctx, &ctx.resolved.scene_break)
}

fn render_block(ctx: &Ctx, block: &Block, indent: &str, chapter: usize) -> String {
    match block {
        Block::Paragraph(spans) => paragraph(indent, &runs(ctx, spans, chapter)),
        Block::Heading { spans, .. } => {
            paragraph(r#"<w:jc w:val="center"/>"#, &runs(ctx, spans, chapter))
        }
        Block::Quote(spans) => paragraph(
            &format!(r#"<w:ind w:left="{}" w:right="{}"/>"#, INCH / 2, INCH / 2),
            &runs(ctx, spans, chapter),
        ),
        Block::SceneBreak => scene_break_para(ctx),
    }
}

fn document(ctx: &Ctx) -> String {
    let single_section = !ctx.sectioned || ctx.m.chapters.len() < 2;
    // The last section's properties are the body's; when there are earlier
    // sections, the first of them carried the header and the title-page rule.
    let last = if single_section {
        format!(
            r#"{}<w:headerReference w:type="default" r:id="rId2"/>
      {}<w:titlePg/>"#,
            "", "",
        )
    } else {
        String::new()
    };
    let _ = last;
    let (refs, title) = if single_section {
        (
            format!(
                "<w:headerReference w:type=\"default\" r:id=\"rId2\"/>{}",
                sect_props(ctx, false)
            ),
            "<w:titlePg/>",
        )
    } else {
        (sect_props(ctx, false), "")
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>
    {front}{contents}
    {body}
    <w:sectPr>
      {refs}
      {title}
      {setup}
    </w:sectPr>
  </w:body>
</w:document>"#,
        front = front_matter(ctx),
        contents = {
            let c = contents(ctx);
            if c.is_empty() {
                c
            } else {
                format!("\n    {c}")
            }
        },
        body = body(ctx),
        setup = page_setup(),
    )
}

fn notes_part(ctx: &Ctx, endnotes: bool) -> String {
    let (root, item, mark, text_style, separators) = if endnotes {
        ("w:endnotes", "w:endnote", "w:endnoteRef", "EndnoteText", "endnote")
    } else {
        ("w:footnotes", "w:footnote", "w:footnoteRef", "FootnoteText", "footnote")
    };
    let _ = separators;
    let separator = |kind: &str, id: i32, inner: &str| {
        format!(
            r#"<{item} w:type="{kind}" w:id="{id}"><w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:r><{inner}/></w:r></w:p></{item}>"#
        )
    };
    let mut out = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<{root} xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  {}
  {}
"#,
        separator("separator", -1, "w:separator"),
        separator("continuationSeparator", 0, "w:continuationSeparator"),
    );
    for (ci, chapter) in ctx.m.chapters.iter().enumerate() {
        for note in &chapter.notes {
            let id = ctx.note_ids[&(ci, note.number)];
            out.push_str(&format!(
                r#"  <{item} w:id="{id}"><w:p><w:pPr><w:pStyle w:val="{text_style}"/></w:pPr><w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><{mark}/></w:r>{}</w:p></{item}>
"#,
                note_runs(note, ctx.m.note_label)
            ));
        }
    }
    out.push_str(&format!("</{root}>"));
    out
}

/// A note's text: an optional label, the passage in italics when asked for,
/// then what the writer wrote.
fn note_runs(note: &Note, label: bool) -> String {
    let t = |text: &str, italic: bool| {
        format!(
            "<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
            if italic { "<w:rPr><w:i/></w:rPr>" } else { "" },
            xml_escape(text)
        )
    };
    let mut out = t(" ", false);
    if label {
        out.push_str(&t("Comment: ", false));
    }
    if let Some(quote) = &note.quote {
        out.push_str(&t(&format!("“{}” — ", quote.trim()), true));
    }
    out.push_str(&t(&note.text, false));
    out
}

fn settings_xml(ctx: &Ctx, hyphenate: bool) -> String {
    let mut body = String::new();
    if hyphenate {
        body.push_str("<w:autoHyphenation/>");
    }
    if ctx.has_notes() {
        if ctx.endnotes() {
            body.push_str(r#"<w:endnotePr><w:endnote w:id="-1"/><w:endnote w:id="0"/></w:endnotePr>"#);
        } else {
            body.push_str(r#"<w:footnotePr><w:footnote w:id="-1"/><w:footnote w:id="0"/></w:footnotePr>"#);
        }
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{body}</w:settings>"#
    )
}

fn core_properties(m: &Manuscript) -> String {
    let mut body = format!(
        "<dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:language>{}</dc:language>",
        xml_escape(&m.title),
        xml_escape(&m.byline),
        xml_escape(m.lang())
    );
    if let Some(description) = &m.description {
        body.push_str(&format!(
            "<dc:description>{}</dc:description>",
            xml_escape(description)
        ));
    }
    if !m.keywords.is_empty() {
        body.push_str(&format!(
            "<cp:keywords>{}</cp:keywords>",
            xml_escape(&m.keywords.join(", "))
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">{body}</cp:coreProperties>"#
    )
}

fn content_types(ctx: &Ctx, details: bool, settings: bool) -> String {
    let mut extra = String::new();
    if settings {
        extra.push_str("\n  <Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>");
    }
    if ctx.has_notes() {
        if ctx.endnotes() {
            extra.push_str("\n  <Override PartName=\"/word/endnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml\"/>");
        } else {
            extra.push_str("\n  <Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>");
        }
    }
    if details {
        extra.push_str("\n  <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    }
    CONTENT_TYPES.replace("</Types>", &format!("{extra}\n</Types>")).replace("\n\n</Types>", "\n</Types>")
}

fn root_rels(details: bool) -> String {
    if !details {
        return ROOT_RELS.to_string();
    }
    ROOT_RELS.replace(
        "</Relationships>",
        "  <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\n</Relationships>",
    )
}

fn document_rels(ctx: &Ctx, settings: bool) -> String {
    let mut extra = String::new();
    if settings {
        extra.push_str("  <Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>\n");
    }
    if ctx.has_notes() {
        if ctx.endnotes() {
            extra.push_str("  <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes\" Target=\"endnotes.xml\"/>\n");
        } else {
            extra.push_str("  <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/>\n");
        }
    }
    DOCUMENT_RELS.replace("</Relationships>", &format!("{extra}</Relationships>"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::options::{Format, StyleOptions, StylePreset};
    use crate::export::{ChapterContent, SceneContent};
    use std::io::Read;

    fn book(chapters: usize) -> Manuscript {
        Manuscript {
            title: "T".into(),
            byline: "A B".into(),
            surname: "B".into(),
            short_title: "T".into(),
            chapters: (1..=chapters)
                .map(|n| ChapterContent {
                    title: format!("Chapter {n}"),
                    scenes: vec![SceneContent {
                        blocks: vec![Block::Paragraph(vec![
                            Span { text: "Some text".into(), ..Default::default() },
                            Span { note: Some(1), ..Default::default() },
                            Span { text: " more.".into(), ..Default::default() },
                        ])],
                        ..Default::default()
                    }],
                    notes: vec![Note { number: 1, text: "a note".into(), quote: Some("Some text".into()) }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn parts(bytes: Vec<u8>) -> std::collections::BTreeMap<String, String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = std::collections::BTreeMap::new();
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let mut body = String::new();
            f.read_to_string(&mut body).unwrap();
            out.insert(f.name().to_string(), body);
        }
        out
    }

    fn opts(style: StylePreset) -> ExportOptions {
        ExportOptions {
            format: Format::Docx,
            style: StyleOptions { preset: style, ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn footnotes_are_real_word_footnotes() {
        let mut m = book(2);
        m.note_layout = NoteLayout::Footnotes;
        let p = parts(write(&m, &opts(StylePreset::Book)).unwrap());
        let doc = &p["word/document.xml"];
        assert!(doc.contains(r#"<w:footnoteReference w:id="1"/>"#));
        assert!(doc.contains(r#"<w:footnoteReference w:id="2"/>"#));
        let notes = &p["word/footnotes.xml"];
        assert!(notes.contains(r#"w:type="separator" w:id="-1""#));
        assert!(notes.contains("a note"));
        assert!(notes.contains("“Some text” — "));
        assert!(p["[Content_Types].xml"].contains("footnotes+xml"));
        assert!(p["word/_rels/document.xml.rels"].contains("footnotes.xml"));
        assert!(p["word/settings.xml"].contains("<w:footnotePr>"));
        assert!(!p.contains_key("word/endnotes.xml"));
    }

    #[test]
    fn endnotes_per_chapter_end_each_section() {
        let mut m = book(2);
        m.note_layout = NoteLayout::EndnotesChapter;
        let mut o = opts(StylePreset::Book);
        o.content.comment_numbering = Numbering::Chapter;
        let p = parts(write(&m, &o).unwrap());
        let doc = &p["word/document.xml"];
        assert!(doc.contains(r#"<w:endnoteReference w:id="2"/>"#));
        assert!(doc.contains(r#"<w:pos w:val="sectEnd"/>"#));
        assert!(doc.contains(r#"<w:numRestart w:val="eachSect"/>"#));
        assert!(p["word/endnotes.xml"].contains("<w:endnote w:id=\"2\">"));
        // The first section carries the header and the title-page rule.
        assert_eq!(doc.matches("<w:titlePg/>").count(), 1);
    }

    #[test]
    fn a_book_without_notes_has_no_note_parts() {
        let mut m = book(2);
        m.note_layout = NoteLayout::None;
        for c in &mut m.chapters {
            c.notes.clear();
        }
        let p = parts(write(&m, &opts(StylePreset::Book)).unwrap());
        assert!(!p.contains_key("word/footnotes.xml"));
        assert!(!p.contains_key("word/settings.xml"));
        assert!(!p.contains_key("docProps/core.xml"));
    }

    #[test]
    fn front_matter_off_and_toc_on() {
        let mut m = book(2);
        m.note_layout = NoteLayout::None;
        m.omit_front_matter = true;
        m.toc = true;
        let p = parts(write(&m, &opts(StylePreset::Book)).unwrap());
        let doc = &p["word/document.xml"];
        assert!(!doc.contains("by A B"));
        assert!(doc.contains(r#"<w:hyperlink w:anchor="chapter2""#));
        assert!(doc.contains(r#"<w:bookmarkStart w:id="2" w:name="chapter2"/>"#));
    }

    #[test]
    fn details_become_core_properties() {
        let mut m = book(1);
        m.description = Some("Desc".into());
        m.language = "fr".into();
        let p = parts(write(&m, &opts(StylePreset::Book)).unwrap());
        let core = &p["docProps/core.xml"];
        assert!(core.contains("<dc:language>fr</dc:language>"));
        assert!(core.contains("<dc:description>Desc</dc:description>"));
        assert!(p["_rels/.rels"].contains("core-properties"));
        assert!(p["word/styles.xml"].contains(r#"<w:lang w:val="fr"/>"#));
    }

    #[test]
    fn book_typography_reaches_styles_and_paragraphs() {
        let mut o = opts(StylePreset::Book);
        o.style.font = Some("garamond".into());
        o.style.size_pt = Some(11.0);
        o.style.align = Some(Align::Left);
        o.style.hyphenate = Some(true);
        o.style.scene_break = Some("* * *".into());
        let mut m = book(1);
        m.note_layout = NoteLayout::None;
        m.chapters[0].notes.clear();
        m.chapters[0].scenes.push(SceneContent {
            blocks: vec![Block::Paragraph(vec![Span { text: "x".into(), ..Default::default() }])],
            ..Default::default()
        });
        let p = parts(write(&m, &o).unwrap());
        assert!(p["word/styles.xml"].contains(r#"w:ascii="Garamond""#));
        assert!(p["word/styles.xml"].contains(r#"<w:sz w:val="22"/>"#));
        assert!(p["word/document.xml"].contains(r#"<w:jc w:val="left"/>"#));
        assert!(p["word/document.xml"].contains("* * *"));
        assert!(p["word/settings.xml"].contains("<w:autoHyphenation/>"));
    }

    #[test]
    fn the_manuscript_preset_ignores_typography() {
        let mut o = opts(StylePreset::Manuscript);
        o.style.font = Some("garamond".into());
        let p = parts(write(&book(1), &o).unwrap());
        assert!(p["word/styles.xml"].contains("Courier New"));
    }

    /// Writes real files for a human, or LibreOffice, to open.
    #[test]
    #[ignore]
    fn dump_samples() {
        let dir = std::env::var("DUMP_DIR").unwrap();
        for (name, layout, restart) in [
            ("foot", NoteLayout::Footnotes, false),
            ("foot-restart", NoteLayout::Footnotes, true),
            ("end-chapter", NoteLayout::EndnotesChapter, true),
            ("end-book", NoteLayout::EndnotesBook, false),
        ] {
            let mut m = book(3);
            m.note_layout = layout;
            m.toc = true;
            let mut o = opts(StylePreset::Book);
            if restart {
                o.content.comment_numbering = Numbering::Chapter;
            }
            std::fs::write(format!("{dir}/{name}.docx"), write(&m, &o).unwrap()).unwrap();
        }
    }
}
