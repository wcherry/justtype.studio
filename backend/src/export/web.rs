//! What HTML and EPUB share: stylesheets generated from the typography
//! options, and the markup for notes and chapter roles.
use super::options::{font_stack, Align, HeadingStyle, ResolvedStyle};
use super::{xml_escape, ChapterContent, Note, Role, Style};

fn css_string(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}

/// Rules that apply the writer's typography on top of a base stylesheet.
/// Empty when nothing was overridden, so a default export is the same file it
/// always was. `h1` is the chapter heading's tag (EPUB uses `h1`, HTML `h2`).
pub fn style_overrides(style: &ResolvedStyle, chapter_heading: &str, rem_relative: bool) -> String {
    if style.preset == Style::Manuscript || !style.customised {
        return String::new();
    }
    let mut css = String::new();
    // An e-reader lets its owner choose the size; a size relative to theirs
    // respects that, where a fixed one would fight it.
    let size = if rem_relative {
        format!("{:.0}%", style.size_pt / 12.0 * 100.0)
    } else {
        format!("{}pt", style.size_pt)
    };
    let align = match style.align {
        Align::Left => "left",
        Align::Justify => "justify",
    };
    css.push_str(&format!(
        "body{{font-family:{};font-size:{};line-height:{};}}\n",
        font_stack(&style.font),
        size,
        style.line_spacing.css()
    ));
    css.push_str(&format!(
        "p{{text-indent:{}em;text-align:{align};margin-bottom:{}em;{}}}\n",
        style.paragraph_indent_em,
        style.paragraph_spacing_em,
        if style.hyphenate {
            "hyphens:auto;-webkit-hyphens:auto;"
        } else {
            "hyphens:manual;-webkit-hyphens:manual;"
        }
    ));
    match style.heading_style {
        HeadingStyle::Plain => {}
        HeadingStyle::SmallCaps => css.push_str(&format!(
            "{chapter_heading}{{font-variant:small-caps;letter-spacing:.05em;}}\n"
        )),
        HeadingStyle::Centred => {
            css.push_str(&format!("{chapter_heading}{{text-align:center;}}\n"))
        }
    }
    css.push_str(&format!(
        "hr.scene::after{{content:\"{}\";}}\n",
        css_string(&style.scene_break)
    ));
    if style.scene_break.is_empty() {
        css.push_str("hr.scene{height:1.5em;}\n");
    }
    if style.drop_caps {
        css.push_str(&format!(
            "{chapter_heading} + p::first-letter{{float:left;font-size:3em;line-height:.85;\
             padding-right:.08em;text-indent:0;}}\n"
        ));
    }
    css
}

/// The `epub:type` that best describes a chapter: its role, or something more
/// specific when its title says what it is.
pub fn epub_type(chapter: &ChapterContent) -> &'static str {
    let title = chapter.title.to_lowercase();
    let named = [
        ("acknowledg", "acknowledgments"),
        ("afterword", "afterword"),
        ("appendix", "appendix"),
        ("foreword", "foreword"),
        ("preface", "preface"),
        ("prologue", "prologue"),
        ("epilogue", "epilogue"),
        ("glossary", "glossary"),
        ("bibliograph", "bibliography"),
        ("about the author", "colophon"),
    ];
    if chapter.role != Role::Chapter {
        if let Some((_, kind)) = named.iter().find(|(key, _)| title.contains(key)) {
            return kind;
        }
    }
    match chapter.role {
        Role::Chapter => "chapter",
        Role::Front => "frontmatter",
        Role::Back => "backmatter",
    }
}

/// How a note reference and its target are spelled in one flavour of markup.
pub struct NoteMarkup {
    /// Prefix that keeps ids unique when numbering restarts each chapter.
    pub scope: String,
    /// The page the notes are on, when that is not the page of the mark.
    pub notes_page: Option<String>,
    pub epub: bool,
    pub classed: bool,
}

impl NoteMarkup {
    pub fn reference(&self, number: u32) -> String {
        let id = format!("{}-{number}", self.scope);
        let page = self.notes_page.as_deref().unwrap_or("");
        if self.epub {
            format!(
                "<sup><a epub:type=\"noteref\" id=\"r{id}\" href=\"{page}#n{id}\">{number}</a></sup>"
            )
        } else {
            format!("<sup><a id=\"fnref{id}\" href=\"{page}#fn{id}\">{number}</a></sup>")
        }
    }

    /// The notes of one chapter (or of the whole book), as a section.
    pub fn section(
        &self,
        notes: &[(&str, &Note)],
        label: bool,
        back_page: impl Fn(&str) -> String,
    ) -> String {
        if notes.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        let class = if self.classed {
            " class=\"footnotes\""
        } else {
            ""
        };
        if self.epub {
            out.push_str(&format!(
                "<section epub:type=\"endnotes\"{class}>\n<h2>Notes</h2>\n"
            ));
        } else {
            out.push_str(&format!("<section{class}>\n<h2>Notes</h2>\n"));
        }
        for (scope, note) in notes {
            let id = format!("{scope}-{}", note.number);
            let mut text = String::new();
            if label {
                text.push_str("Comment: ");
            }
            if let Some(quote) = &note.quote {
                text.push_str(&format!("“{}” — ", quote.trim()));
            }
            text.push_str(&note.text);
            let back = back_page(scope);
            if self.epub {
                out.push_str(&format!(
                    "<aside epub:type=\"footnote\" id=\"n{id}\"><p><sup>{}</sup> {} \
                     <a href=\"{back}#r{id}\">↩</a></p></aside>\n",
                    note.number,
                    xml_escape(&text)
                ));
            } else {
                out.push_str(&format!(
                    "<p id=\"fn{id}\"><sup>{}</sup> {} <a href=\"{back}#fnref{id}\">↩</a></p>\n",
                    note.number,
                    xml_escape(&text)
                ));
            }
        }
        out.push_str("</section>\n");
        out
    }
}
