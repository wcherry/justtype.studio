//! Everything a writer can ask of an export, in three layers.
//!
//! *Content* says what goes in the file, *style* says how the text looks, and
//! the per-format blocks carry settings only one format understands. The
//! layers keep PDF page settings out of EPUB and EPUB settings out of PDF, and
//! they mean each later feature only adds fields.
//!
//! Every field has a default, and the defaults are today's output: an empty
//! JSON body is the same export the old `?format=` query gave.
use serde::{Deserialize, Serialize};

use super::Style;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Docx,
    Pdf,
    Epub,
    Html,
    Txt,
}

impl Format {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "docx" => Some(Self::Docx),
            "pdf" => Some(Self::Pdf),
            "epub" => Some(Self::Epub),
            "html" => Some(Self::Html),
            "txt" => Some(Self::Txt),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Pdf => "pdf",
            Self::Epub => "epub",
            Self::Html => "html",
            Self::Txt => "txt",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Pdf => "application/pdf",
            Self::Epub => "application/epub+zip",
            Self::Html => "text/html; charset=utf-8",
            Self::Txt => "text/plain; charset=utf-8",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    #[default]
    All,
    Selected,
}

/// Which chapters and scenes go in.
///
/// A flat object rather than a tagged union, so every client can model it
/// without one: `kind` says whether the ids count.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Scope {
    pub kind: ScopeKind,
    /// A chapter id selects the chapter and every scene in it.
    pub chapter_ids: Vec<String>,
    /// A scene id on its own selects just that scene (under its chapter's
    /// title).
    pub scene_ids: Vec<String>,
}

impl Scope {
    #[cfg(test)]
    pub fn selected(chapter_ids: Vec<String>, scene_ids: Vec<String>) -> Self {
        Self {
            kind: ScopeKind::Selected,
            chapter_ids,
            scene_ids,
        }
    }
    pub fn is_selection(&self) -> bool {
        self.kind == ScopeKind::Selected
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Numbering {
    #[default]
    Continuous,
    Chapter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NotePlacement {
    #[default]
    Chapter,
    Book,
}

/// What to do with the writer's own comments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum CommentsMode {
    #[default]
    Off,
    Footnotes,
    Endnotes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ContentOptions {
    pub scope: Scope,
    pub front_matter: bool,
    /// `None` takes the format's own default: on for EPUB, whose navigation
    /// document is required, and off elsewhere.
    pub toc: Option<bool>,
    /// `None` takes the preset's default: off for manuscript, on for book.
    pub back_matter: Option<bool>,
    pub comments: CommentsMode,
    /// For endnotes: at the end of each chapter, or of the book.
    pub note_placement: NotePlacement,
    pub comment_numbering: Numbering,
    /// `None`: off for footnotes, on for endnotes, where the note is far from
    /// its passage.
    pub quote_passage: Option<bool>,
    /// Prefix each note with "Comment:".
    pub comment_label: bool,
    /// Append the writer's *About the author* text as back matter.
    pub about_author: bool,
}

impl Default for ContentOptions {
    fn default() -> Self {
        Self {
            scope: Scope::default(),
            front_matter: true,
            toc: None,
            back_matter: None,
            comments: CommentsMode::Off,
            note_placement: NotePlacement::Chapter,
            comment_numbering: Numbering::Continuous,
            quote_passage: None,
            comment_label: false,
            about_author: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LineSpacing {
    Single,
    #[serde(rename = "1.15")]
    OnePointOneFive,
    #[serde(rename = "1.5")]
    OnePointFive,
    Double,
}

impl LineSpacing {
    /// Twips (240 = single) for Word's `w:line`.
    pub fn twips(self) -> i32 {
        match self {
            Self::Single => 240,
            Self::OnePointOneFive => 276,
            Self::OnePointFive => 360,
            Self::Double => 480,
        }
    }
    /// The multiple of the type size between baselines.
    pub fn factor(self) -> f32 {
        match self {
            Self::Single => 1.2,
            Self::OnePointOneFive => 1.35,
            Self::OnePointFive => 1.65,
            Self::Double => 2.1,
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            Self::Single => "1.2",
            Self::OnePointOneFive => "1.15",
            Self::OnePointFive => "1.5",
            Self::Double => "2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum HeadingStyle {
    #[default]
    Plain,
    SmallCaps,
    Centred,
}

/// A curated list: faces we can name in a file and expect a reader to have
/// (or to fall back from sensibly).
pub const FONTS: &[(&str, &str)] = &[
    ("georgia", "Georgia"),
    ("garamond", "Garamond"),
    ("palatino", "Palatino Linotype"),
    ("times", "Times New Roman"),
    ("baskerville", "Baskerville"),
    ("helvetica", "Helvetica"),
    ("verdana", "Verdana"),
    ("courier", "Courier New"),
];

/// The CSS `font-family` for a face by its display name, with a generic
/// fallback so a reader without it still gets the right kind of type.
pub fn font_stack(name: &str) -> String {
    let generic = match name {
        "Helvetica" | "Verdana" => "sans-serif",
        "Courier New" => "monospace",
        _ => "serif",
    };
    match name {
        "Helvetica" => format!("Helvetica, Arial, {generic}"),
        "Palatino Linotype" => format!("'Palatino Linotype', Palatino, {generic}"),
        "Times New Roman" => format!("'Times New Roman', Times, {generic}"),
        "Garamond" => format!("Garamond, 'EB Garamond', {generic}"),
        "Courier New" => format!("'Courier New', Courier, {generic}"),
        other => format!("{other}, {generic}"),
    }
}

/// Typography. A preset plus overrides on top of it; `None` means the preset's
/// own value. The manuscript preset ignores every override: fixing them is the
/// point of the format.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct StyleOptions {
    pub preset: StylePreset,
    /// A key from [`FONTS`].
    pub font: Option<String>,
    pub size_pt: Option<f32>,
    pub line_spacing: Option<LineSpacing>,
    pub align: Option<Align>,
    /// First-line indent, in ems.
    pub paragraph_indent_em: Option<f32>,
    /// Space between paragraphs, in ems.
    pub paragraph_spacing_em: Option<f32>,
    pub heading_style: Option<HeadingStyle>,
    /// `⁂`, `* * *`, `#`, an empty string for a blank line, or custom text.
    pub scene_break: Option<String>,
    pub drop_caps: Option<bool>,
    pub hyphenate: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum StylePreset {
    #[default]
    Manuscript,
    Book,
}

impl From<StylePreset> for Style {
    fn from(preset: StylePreset) -> Style {
        match preset {
            StylePreset::Manuscript => Style::Manuscript,
            StylePreset::Book => Style::Book,
        }
    }
}

/// The typography a writer will actually apply, with every override resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStyle {
    pub preset: Style,
    pub font: String,
    pub size_pt: f32,
    pub line_spacing: LineSpacing,
    pub align: Align,
    pub paragraph_indent_em: f32,
    pub paragraph_spacing_em: f32,
    pub heading_style: HeadingStyle,
    pub scene_break: String,
    pub drop_caps: bool,
    pub hyphenate: bool,
    /// True when any override was honoured (so a writer can skip emitting
    /// anything extra, and so today's output stays byte for byte).
    pub customised: bool,
}

impl StyleOptions {
    pub fn resolve(&self) -> ResolvedStyle {
        let preset: Style = self.preset.into();
        let fixed = preset == Style::Manuscript;
        fn pick<T>(fixed: bool, over: Option<T>, base: T) -> T {
            if fixed {
                base
            } else {
                over.unwrap_or(base)
            }
        }

        let font = if fixed {
            "Courier New".to_string()
        } else {
            self.font
                .as_deref()
                .and_then(|key| FONTS.iter().find(|(k, _)| *k == key))
                .map(|(_, name)| name.to_string())
                .unwrap_or_else(|| "Georgia".to_string())
        };

        ResolvedStyle {
            preset,
            font,
            size_pt: pick(fixed, self.size_pt, 12.0),
            line_spacing: pick(
                fixed,
                self.line_spacing,
                if fixed {
                    LineSpacing::Double
                } else {
                    LineSpacing::OnePointOneFive
                },
            ),
            align: pick(
                fixed,
                self.align,
                if fixed { Align::Left } else { Align::Justify },
            ),
            paragraph_indent_em: pick(fixed, self.paragraph_indent_em, 1.5),
            paragraph_spacing_em: pick(fixed, self.paragraph_spacing_em, 0.0),
            heading_style: pick(fixed, self.heading_style, HeadingStyle::Plain),
            scene_break: if fixed {
                "#".to_string()
            } else {
                self.scene_break.clone().unwrap_or_else(|| "⁂".to_string())
            },
            drop_caps: pick(fixed, self.drop_caps, false),
            hyphenate: pick(fixed, self.hyphenate, false),
            customised: !fixed
                && *self
                    != StyleOptions {
                        preset: self.preset,
                        ..Default::default()
                    },
        }
    }

    fn validate(&self) -> Result<(), String> {
        if let Some(size) = self.size_pt {
            if !(8.0..=24.0).contains(&size) {
                return Err("style.sizePt must be between 8 and 24".into());
            }
        }
        if let Some(key) = &self.font {
            if !FONTS.iter().any(|(k, _)| k == key) {
                let known: Vec<&str> = FONTS.iter().map(|(k, _)| *k).collect();
                return Err(format!(
                    "style.font `{key}` is not one of {}",
                    known.join(", ")
                ));
            }
        }
        for (name, value) in [
            ("paragraphIndentEm", self.paragraph_indent_em),
            ("paragraphSpacingEm", self.paragraph_spacing_em),
        ] {
            if let Some(v) = value {
                if !(0.0..=4.0).contains(&v) {
                    return Err(format!("style.{name} must be between 0 and 4"));
                }
            }
        }
        if let Some(glyph) = &self.scene_break {
            if glyph.chars().count() > 40 {
                return Err("style.sceneBreak is limited to 40 characters".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageSize {
    Letter,
    A4,
    Trade5x8,
    Trade5_5x8_5,
    Trade6x9,
}

impl PageSize {
    /// Width and height in points.
    pub fn points(self) -> (f32, f32) {
        match self {
            Self::Letter => (612.0, 792.0),
            Self::A4 => (595.0, 842.0),
            Self::Trade5x8 => (360.0, 576.0),
            Self::Trade5_5x8_5 => (396.0, 612.0),
            Self::Trade6x9 => (432.0, 648.0),
        }
    }
}

/// Where the page number sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FolioPosition {
    #[default]
    BottomCentre,
    /// At the foot, on the outside edge: left on a left-hand page.
    BottomOutside,
    /// In the head line, on the outside edge; the running head is centred.
    TopOutside,
}

/// How the page number is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FolioStyle {
    /// `12`
    #[default]
    Plain,
    /// `– 12 –`
    Dashes,
}

/// Page setup for PDF. Book-layout PDF reads these; manuscript PDF is fixed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct PdfOptions {
    pub page_size: Option<PageSize>,
    /// Inches; `None` is 1. The top, bottom and outside edges.
    pub margin_in: Option<f32>,
    /// The inside (binding) edge, mirrored on facing pages. `None` is a little
    /// more than the margin, for the gutter.
    pub inside_margin_in: Option<f32>,
    pub landscape: bool,
    /// Page numbers on the pages of the book (not on chapter openings).
    pub page_numbers: Option<bool>,
    pub page_number_position: FolioPosition,
    pub page_number_style: FolioStyle,
    pub start_page: Option<u32>,
    pub roman_front_matter: bool,
    pub chapters_on_right: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct EpubOptions {
    /// One XHTML file per chapter (the default) or the whole book in one.
    pub split_chapters: bool,
    /// Also a cover page in the spine, not just the cover image.
    pub cover_page: bool,
}

impl Default for EpubOptions {
    fn default() -> Self {
        Self {
            split_chapters: true,
            cover_page: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum HtmlMode {
    /// One page with a stylesheet inline: today's output.
    #[default]
    Styled,
    /// Semantic markup only, no classes or styles, for pasting into a CMS.
    Clean,
    /// A zip: `index.html`, a page per chapter, `style.css`, the cover.
    Package,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct HtmlOptions {
    pub mode: HtmlMode,
}

/// Export-time replacements for the work's own metadata. They apply to this
/// export only: a pen name for an EPUB, while the project keeps the writer's.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct MetadataOverrides {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub language: Option<String>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub identifier: Option<String>,
    pub published: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct ExportOptions {
    pub format: Format,
    pub content: ContentOptions,
    pub style: StyleOptions,
    pub metadata: MetadataOverrides,
    pub pdf: Option<PdfOptions>,
    pub epub: Option<EpubOptions>,
    pub html: Option<HtmlOptions>,
    /// `{title}`, `{short_title}`, `{author}`, `{date}`, `{version}`,
    /// `{format}`. Unset is the title.
    pub filename_template: Option<String>,
    /// The profile these options came from, for the export history.
    pub profile: Option<String>,
}

/// The placeholders a filename template may use.
pub const FILENAME_FIELDS: &[&str] = &[
    "title",
    "short_title",
    "author",
    "date",
    "version",
    "format",
];

/// The placeholders in a template, or the first unknown one.
pub fn template_fields(template: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err("filenameTemplate has a `{` with no closing `}`".into());
        };
        let name = &after[..end];
        if !FILENAME_FIELDS.contains(&name) {
            return Err(format!(
                "filenameTemplate uses `{{{name}}}`, which is not one of {}",
                FILENAME_FIELDS
                    .iter()
                    .map(|f| format!("{{{f}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        fields.push(name.to_string());
        rest = &after[end + 1..];
    }
    Ok(fields)
}

/// Fills a template in. Unknown placeholders are left as they are; validation
/// refuses them before this is reached.
pub fn fill_template(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

impl ExportOptions {
    /// Brings options saved earlier up to date: anything that no longer exists
    /// (a font that was withdrawn, a setting that was renamed) falls back to
    /// its default, and what was changed is reported so the writer is told
    /// rather than surprised.
    pub fn repaired(value: serde_json::Value) -> (Self, Vec<String>) {
        let mut notes = Vec::new();
        let mut parsed: Self = match serde_json::from_value(value.clone()) {
            Ok(options) => options,
            Err(_) => {
                // Take it a section at a time, keeping what still parses.
                let mut options = Self::default();
                if let Some(object) = value.as_object() {
                    macro_rules! section {
                        ($key:literal, $field:ident, $ty:ty) => {
                            if let Some(v) = object.get($key) {
                                match serde_json::from_value::<$ty>(v.clone()) {
                                    Ok(parsed) => options.$field = parsed,
                                    Err(_) => notes.push(format!(
                                        "The saved `{}` settings are out of date and were reset.",
                                        $key
                                    )),
                                }
                            }
                        };
                    }
                    section!("format", format, Format);
                    section!("content", content, ContentOptions);
                    section!("style", style, StyleOptions);
                    section!("metadata", metadata, MetadataOverrides);
                    section!("pdf", pdf, Option<PdfOptions>);
                    section!("epub", epub, Option<EpubOptions>);
                    section!("html", html, Option<HtmlOptions>);
                    section!("filenameTemplate", filename_template, Option<String>);
                }
                options
            }
        };
        if let Some(font) = parsed.style.font.clone() {
            if !FONTS.iter().any(|(k, _)| *k == font) {
                notes.push(format!(
                    "The font `{font}` is no longer offered, so the default is used."
                ));
                parsed.style.font = None;
            }
        }
        // Settings that belong to another format cannot be honoured; drop
        // them and say so, rather than refuse the whole profile.
        for (field, present, wanted) in [
            ("pdf", parsed.pdf.is_some(), Format::Pdf),
            ("epub", parsed.epub.is_some(), Format::Epub),
            ("html", parsed.html.is_some(), Format::Html),
        ] {
            if present && parsed.format != wanted {
                notes.push(format!(
                    "The `{field}` settings do not apply to this format and were dropped."
                ));
                match field {
                    "pdf" => parsed.pdf = None,
                    "epub" => parsed.epub = None,
                    _ => parsed.html = None,
                }
            }
        }
        if let Some(template) = &parsed.filename_template {
            if template_fields(template).is_err() {
                notes.push("The saved filename template is out of date and was reset.".into());
                parsed.filename_template = None;
            }
        }
        (parsed, notes)
    }

    /// A profile is presentation, not a snapshot of the manuscript: the
    /// selection and the metadata overrides stay behind.
    pub fn as_profile(&self) -> Self {
        let mut profile = self.clone();
        profile.content.scope = Scope::default();
        profile.metadata = MetadataOverrides::default();
        profile.profile = None;
        profile
    }

    /// What the old `?format=&style=` query meant.
    pub fn shorthand(format: Format, style: Style) -> Self {
        Self {
            format,
            style: StyleOptions {
                preset: match style {
                    Style::Manuscript => StylePreset::Manuscript,
                    Style::Book => StylePreset::Book,
                },
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Refuses settings that make no sense together, naming the field. Quietly
    /// ignoring them would hand the writer a file that is not what they asked
    /// for.
    pub fn validate(&self) -> Result<(), String> {
        for (field, present, wanted) in [
            ("pdf", self.pdf.is_some(), Format::Pdf),
            ("epub", self.epub.is_some(), Format::Epub),
            ("html", self.html.is_some(), Format::Html),
        ] {
            if present && self.format != wanted {
                return Err(format!(
                    "`{field}` options only apply to {} exports, but format is `{}`",
                    wanted.extension(),
                    self.format.extension()
                ));
            }
        }
        self.style.validate()?;
        if let Some(template) = &self.filename_template {
            template_fields(template)?;
            if template.len() > 200 {
                return Err("filenameTemplate is limited to 200 characters".into());
            }
        }
        let scope = &self.content.scope;
        if scope.chapter_ids.len() + scope.scene_ids.len() > 5_000 {
            return Err("content.scope selects too many items".into());
        }
        if let Some(pdf) = &self.pdf {
            for (name, value) in [
                ("marginIn", pdf.margin_in),
                ("insideMarginIn", pdf.inside_margin_in),
            ] {
                if let Some(margin) = value {
                    if !(0.25..=3.0).contains(&margin) {
                        return Err(format!("pdf.{name} must be between 0.25 and 3"));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn style_kind(&self) -> Style {
        self.style.preset.into()
    }

    pub fn include_toc(&self) -> bool {
        self.content.toc.unwrap_or(self.format == Format::Epub)
    }

    pub fn include_back_matter(&self) -> bool {
        self.content
            .back_matter
            .unwrap_or(self.style.preset == StylePreset::Book)
    }

    pub fn quote_passage(&self) -> bool {
        self.content
            .quote_passage
            .unwrap_or(self.content.comments == CommentsMode::Endnotes)
    }

    pub fn epub(&self) -> EpubOptions {
        self.epub.clone().unwrap_or_default()
    }

    pub fn html(&self) -> HtmlOptions {
        self.html.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_empty_body_is_the_default_export() {
        let options: ExportOptions = serde_json::from_value(json!({})).unwrap();
        assert_eq!(options, ExportOptions::default());
        assert_eq!(options.format, Format::Docx);
        assert_eq!(options.style_kind(), Style::Manuscript);
        assert!(options.content.front_matter);
        assert_eq!(options.content.scope, Scope::default());
        assert!(options.validate().is_ok());
    }

    #[test]
    fn defaults_resolve_per_format() {
        let mut options = ExportOptions::default();
        assert!(!options.include_toc());
        options.format = Format::Epub;
        assert!(options.include_toc());
        options.content.toc = Some(false);
        assert!(!options.include_toc());
        assert!(!options.include_back_matter());
        options.style.preset = StylePreset::Book;
        assert!(options.include_back_matter());
    }

    #[test]
    fn options_for_another_format_are_refused_by_name() {
        let options = ExportOptions {
            format: Format::Epub,
            pdf: Some(PdfOptions::default()),
            ..Default::default()
        };
        let error = options.validate().unwrap_err();
        assert!(error.contains("`pdf`"), "{error}");

        let options = ExportOptions {
            format: Format::Html,
            epub: Some(EpubOptions::default()),
            ..Default::default()
        };
        assert!(options.validate().unwrap_err().contains("`epub`"));
    }

    #[test]
    fn unknown_fields_are_not_silently_ignored() {
        let parsed = serde_json::from_value::<ExportOptions>(json!({"fromat": "pdf"}));
        assert!(parsed.is_err());
    }

    #[test]
    fn style_overrides_are_validated() {
        let mut options = ExportOptions::default();
        options.style.size_pt = Some(40.0);
        assert!(options.validate().unwrap_err().contains("sizePt"));
        options.style.size_pt = Some(11.0);
        options.style.font = Some("comic".into());
        assert!(options.validate().unwrap_err().contains("font"));
        options.style.font = Some("garamond".into());
        assert!(options.validate().is_ok());
    }

    #[test]
    fn the_manuscript_preset_ignores_overrides() {
        let style = StyleOptions {
            preset: StylePreset::Manuscript,
            font: Some("garamond".into()),
            size_pt: Some(20.0),
            scene_break: Some("***".into()),
            ..Default::default()
        };
        let resolved = style.resolve();
        assert_eq!(resolved.font, "Courier New");
        assert_eq!(resolved.size_pt, 12.0);
        assert_eq!(resolved.scene_break, "#");
        assert_eq!(resolved.line_spacing, LineSpacing::Double);
        assert!(!resolved.customised);
    }

    #[test]
    fn the_book_preset_applies_overrides() {
        let style = StyleOptions {
            preset: StylePreset::Book,
            font: Some("garamond".into()),
            size_pt: Some(11.0),
            scene_break: Some("* * *".into()),
            line_spacing: Some(LineSpacing::OnePointFive),
            ..Default::default()
        };
        let resolved = style.resolve();
        assert_eq!(resolved.font, "Garamond");
        assert_eq!(resolved.size_pt, 11.0);
        assert_eq!(resolved.scene_break, "* * *");
        assert!(resolved.customised);

        let plain = StyleOptions {
            preset: StylePreset::Book,
            ..Default::default()
        }
        .resolve();
        assert_eq!(plain.font, "Georgia");
        assert_eq!(plain.scene_break, "⁂");
        assert!(!plain.customised);
    }

    #[test]
    fn scope_and_comments_parse_from_json() {
        let options: ExportOptions = serde_json::from_value(json!({
            "format": "html",
            "content": {
                "scope": {"kind": "selected", "chapterIds": ["a"], "sceneIds": []},
                "comments": "endnotes",
                "notePlacement": "book"
            }
        }))
        .unwrap();
        assert!(options.content.scope.is_selection());
        assert_eq!(options.content.comments, CommentsMode::Endnotes);
        assert_eq!(options.content.note_placement, NotePlacement::Book);
        assert!(options.quote_passage());
    }
}
