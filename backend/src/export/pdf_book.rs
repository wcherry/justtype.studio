//! Book-layout PDF: proportional type, justified text, running headers, page
//! numbers, a contents page with real page numbers, bookmarks, and the
//! typeface embedded in the file.
//!
//! The engine is hand-rolled, like the manuscript writer beside it, and for
//! the same reason: the work is small enough that a library would be more to
//! audit than code to write. What it adds is real font metrics (`font`), a
//! line breaker that knows how wide every glyph is and which pairs kern, and
//! pagination that avoids widows and orphans. Manuscript PDF stays on
//! `pdf.rs`, which this does not touch.
//!
//! Known limits, all reported rather than hidden: left-to-right scripts only
//! and the Latin, Greek and Cyrillic the bundled faces cover; hyphenation
//! follows TeX's patterns for the book's language, and a language without
//! them is not hyphenated; drop caps are not applied; footnotes are set as
//! notes at the end of each chapter.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::font::{font, Face, Family, Font};
use super::image::{self, PdfImage};
use super::options::{
    Align, ExportOptions, FolioPosition, FolioStyle, HeadingStyle, PageSize, ResolvedStyle,
};
use super::{Block, Manuscript, NoteLayout, Span};

// ── Shaping ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Glyph {
    gid: u16,
    ch: char,
    /// Advance in font units.
    advance: u16,
    /// Kerning against the glyph before it, in font units (negative pulls in).
    kern: i16,
}

#[derive(Debug, Clone)]
struct Part {
    family: Family,
    face: Face,
    /// Relative to the line's size; superscripts are smaller.
    scale: f32,
    /// Raised by this fraction of the size.
    rise: f32,
    glyphs: Vec<Glyph>,
    text: String,
}

impl Part {
    fn width(&self, size: f32) -> f32 {
        let f = font(self.family, self.face);
        let units: i32 = self
            .glyphs
            .iter()
            .map(|g| g.advance as i32 + g.kern as i32)
            .sum();
        units as f32 * size * self.scale / f.units_per_em as f32
    }
}

fn shape(text: &str, family: Family, face: Face) -> Vec<Glyph> {
    let f = font(family, face);
    let mut out: Vec<Glyph> = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let ch = match ch {
            '\u{00A0}' | '\u{2007}' | '\u{202F}' => ' ',
            '\u{200B}' | '\u{00AD}' | '\r' | '\t' => continue,
            c => c,
        };
        let gid = f.glyph(ch).or_else(|| f.glyph('?')).unwrap_or(0);
        let kern = out.last().map_or(0, |prev| f.kerning(prev.gid, gid));
        out.push(Glyph {
            gid,
            ch,
            advance: f.advance(gid),
            kern,
        });
    }
    out
}

fn part(text: &str, family: Family, face: Face) -> Part {
    Part {
        family,
        face,
        scale: 1.0,
        rise: 0.0,
        glyphs: shape(text, family, face),
        text: text.to_string(),
    }
}

#[derive(Debug, Clone)]
enum Atom {
    Word(Vec<Part>),
    Space,
    /// A line break the writer asked for.
    Break,
}

fn space_width(family: Family, size: f32) -> f32 {
    let f = font(family, Face::Regular);
    f.advance(f.glyph(' ').unwrap_or(0)) as f32 * size / f.units_per_em as f32
}

fn word_width(parts: &[Part], size: f32) -> f32 {
    parts.iter().map(|p| p.width(size)).sum()
}

/// How a block of text is turned into atoms: words, spaces and breaks.
struct Ink {
    family: Family,
    /// Everything is set italic (a block quotation).
    italic: bool,
    bold: bool,
}

fn atoms_of(spans: &[Span], ink: &Ink) -> Vec<Atom> {
    let mut atoms: Vec<Atom> = Vec::new();
    let mut word: Vec<Part> = Vec::new();
    let flush = |word: &mut Vec<Part>, atoms: &mut Vec<Atom>| {
        if !word.is_empty() {
            atoms.push(Atom::Word(std::mem::take(word)));
        }
    };
    for span in spans {
        if let Some(number) = span.note {
            // A reference mark: a small raised number, glued to the word.
            let mut p = part(&number.to_string(), ink.family, Face::of(ink.bold, false));
            p.scale = 0.65;
            p.rise = 0.38;
            word.push(p);
            continue;
        }
        // Emphasis inside an italic block reads as roman, the way a
        // typesetter would turn it.
        let face = Face::of(ink.bold || span.bold, ink.italic != span.italic);
        let mut current = String::new();
        let push = |current: &mut String, word: &mut Vec<Part>| {
            if !current.is_empty() {
                word.push(part(current, ink.family, face));
                current.clear();
            }
        };
        for ch in span.text.chars() {
            if ch == '\n' {
                push(&mut current, &mut word);
                flush(&mut word, &mut atoms);
                atoms.push(Atom::Break);
            } else if ch.is_whitespace() && ch != '\u{00A0}' {
                push(&mut current, &mut word);
                flush(&mut word, &mut atoms);
                if !matches!(atoms.last(), Some(Atom::Space) | Some(Atom::Break) | None) {
                    atoms.push(Atom::Space);
                }
            } else {
                current.push(ch);
            }
        }
        push(&mut current, &mut word);
    }
    flush(&mut word, &mut atoms);
    while matches!(atoms.last(), Some(Atom::Space)) {
        atoms.pop();
    }
    atoms
}

// ── Hyphenation ─────────────────────────────────────────────────────────────

/// The TeX patterns for a BCP 47 tag, if there are any. `en` alone is US
/// English; the Commonwealth variants take the British patterns; German uses
/// the 1996 orthography unless it is Swiss.
fn hyphenation_language(tag: &str) -> Option<hyphenation::Language> {
    use hyphenation::Language;
    let tag = tag.trim().to_ascii_lowercase().replace('_', "-");
    if let Some(language) = Language::try_from_code(&tag) {
        return Some(language);
    }
    let mut subtags = tag.split('-');
    let primary = subtags.next().unwrap_or_default();
    let region = subtags.next().unwrap_or_default();
    let code = match (primary, region) {
        ("en", "gb" | "au" | "nz" | "ie" | "za" | "in") => "en-gb",
        ("en", _) => "en-us",
        ("de", "ch" | "li") => "de-ch-1901",
        ("de", _) => "de-1996",
        ("el", _) => "el-monoton",
        ("no", _) => "nb",
        ("mn", _) => "mn-cyrl",
        ("sr", "latn") => "sh-latn",
        ("sr", _) => "sr-cyrl",
        _ => primary,
    };
    Language::try_from_code(code)
}

/// Whether a book in this language can be hyphenated.
pub fn can_hyphenate(tag: &str) -> bool {
    hyphenation_language(tag).is_some()
}

/// Each language's dictionary, loaded the first time it is wanted.
fn dictionary(language: hyphenation::Language) -> Option<std::sync::Arc<hyphenation::Standard>> {
    use hyphenation::Load;
    use std::sync::{Arc, Mutex, OnceLock};
    type Cache = Mutex<HashMap<hyphenation::Language, Option<Arc<hyphenation::Standard>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache
        .entry(language)
        .or_insert_with(|| {
            hyphenation::Standard::from_embedded(language)
                .ok()
                .map(Arc::new)
        })
        .clone()
}

/// Where a word may be broken, as character indices into the word, by Liang's
/// patterns for the book's language. Nothing for a language without patterns,
/// for short words, and for anything that is not all letters.
pub fn hyphen_points(word: &str, language: &str) -> Vec<usize> {
    use hyphenation::Hyphenator;
    if word.chars().count() < 5 || !word.chars().all(char::is_alphabetic) {
        return Vec::new();
    }
    let Some(dict) = hyphenation_language(language).and_then(dictionary) else {
        return Vec::new();
    };
    // The patterns are lower case. A letter whose lower case is a different
    // length would shift every index after it, so such a word is left whole.
    let lower = word.to_lowercase();
    if lower.chars().count() != word.chars().count() {
        return Vec::new();
    }
    let breaks = dict.hyphenate(&lower).breaks;
    breaks
        .into_iter()
        .map(|byte| lower[..byte].chars().count())
        .collect()
}

// ── Lines ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Justified,
    Left,
    Centre,
    Right,
}

#[derive(Debug, Clone)]
struct Line {
    atoms: Vec<Atom>,
    size: f32,
    leading: f32,
    /// First-line indent or block indent, in points.
    indent: f32,
    /// The room the line was broken to fit.
    measure: f32,
    place: Place,
    /// Extra width given to each space, for a justified line.
    stretch: f32,
    family: Family,
    /// A bold, larger or otherwise set-apart line keeps its own look.
    kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Head,
    Note,
    Contents,
}

fn natural_width(atoms: &[Atom], size: f32, family: Family) -> f32 {
    let space = space_width(family, size);
    atoms
        .iter()
        .map(|a| match a {
            Atom::Word(parts) => word_width(parts, size),
            Atom::Space => space,
            Atom::Break => 0.0,
        })
        .sum()
}

struct Wrap<'a> {
    size: f32,
    leading: f32,
    family: Family,
    first_indent: f32,
    indent: f32,
    measure: f32,
    place: Place,
    hyphenate: bool,
    /// The book's language, for its hyphenation patterns.
    language: &'a str,
    kind: Kind,
}

/// Greedy breaking: fit as many words as the measure allows, break at the
/// last space, and (when asked) split the word that overflows at a hyphen.
fn wrap(atoms: Vec<Atom>, w: &Wrap) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut current: Vec<Atom> = Vec::new();
    let mut width = 0.0f32;
    let space = space_width(w.family, w.size);
    let mut first = true;
    let hyphen = shape("-", w.family, Face::Regular);
    let hyphen_width = {
        let f = font(w.family, Face::Regular);
        hyphen.iter().map(|g| g.advance as f32).sum::<f32>() * w.size / f.units_per_em as f32
    };

    let room = |first: bool| -> f32 { w.measure - if first { w.first_indent } else { w.indent } };
    let finish =
        |current: &mut Vec<Atom>, lines: &mut Vec<Line>, first: &mut bool, forced: bool| {
            while matches!(current.last(), Some(Atom::Space)) {
                current.pop();
            }
            let indent = if *first { w.first_indent } else { w.indent };
            let spaces = current.iter().filter(|a| matches!(a, Atom::Space)).count();
            let natural = natural_width(current, w.size, w.family);
            let avail = w.measure - indent;
            // A justified line stretches its spaces to the measure — all but the
            // last line of a paragraph, which keeps its natural width.
            let stretch = if w.place == Place::Justified && !forced && spaces > 0 && natural < avail
            {
                (avail - natural) / spaces as f32
            } else {
                0.0
            };
            lines.push(Line {
                atoms: std::mem::take(current),
                size: w.size,
                leading: w.leading,
                indent,
                measure: w.measure,
                place: if w.place == Place::Justified && stretch == 0.0 {
                    Place::Left
                } else {
                    w.place
                },
                stretch,
                family: w.family,
                kind: w.kind,
            });
            *first = false;
        };

    let mut queue: std::collections::VecDeque<Atom> = atoms.into();
    while let Some(atom) = queue.pop_front() {
        match atom {
            Atom::Break => {
                finish(&mut current, &mut lines, &mut first, true);
                width = 0.0;
            }
            Atom::Space => {
                if !current.is_empty() {
                    current.push(Atom::Space);
                    width += space;
                }
            }
            Atom::Word(parts) => {
                let ww = word_width(&parts, w.size);
                if width + ww <= room(first) + 0.01 || current.is_empty() && ww <= room(first) {
                    current.push(Atom::Word(parts));
                    width += ww;
                    continue;
                }
                // Does not fit. Try to split it at a hyphen.
                let mut placed = false;
                if w.hyphenate && parts.len() == 1 {
                    let text = parts[0].text.clone();
                    // Punctuation hugging the word stays with its half.
                    let core_start = text.chars().take_while(|c| !c.is_alphabetic()).count();
                    let core: String = text
                        .chars()
                        .skip(core_start)
                        .take_while(|c| c.is_alphabetic())
                        .collect();
                    let core_len = core.chars().count();
                    let points = hyphen_points(&core, w.language);
                    let left_room = room(first) - width;
                    for &at in points.iter().rev() {
                        let cut = core_start + at;
                        let head: String = text.chars().take(cut).collect();
                        let tail: String = text.chars().skip(cut).collect();
                        let head_part = part(&format!("{head}-"), parts[0].family, parts[0].face);
                        if word_width(std::slice::from_ref(&head_part), w.size) <= left_room + 0.01
                            && core_len > at
                        {
                            current.push(Atom::Word(vec![head_part]));
                            finish(&mut current, &mut lines, &mut first, false);
                            width = 0.0;
                            queue.push_front(Atom::Word(vec![part(
                                &tail,
                                parts[0].family,
                                parts[0].face,
                            )]));
                            placed = true;
                            break;
                        }
                    }
                    let _ = hyphen_width;
                }
                if placed {
                    continue;
                }
                if !current.is_empty() {
                    finish(&mut current, &mut lines, &mut first, false);
                    width = 0.0;
                    queue.push_front(Atom::Word(parts));
                    continue;
                }
                // A word wider than the whole line: break it where it must.
                let mut rest = parts;
                let mut taken: Vec<Part> = Vec::new();
                let mut used = 0.0;
                while let Some(mut p) = (!rest.is_empty()).then(|| rest.remove(0)) {
                    let mut head = Part {
                        glyphs: Vec::new(),
                        text: String::new(),
                        ..p.clone()
                    };
                    let f = font(p.family, p.face);
                    let mut idx = 0;
                    for g in &p.glyphs {
                        let adv = (g.advance as f32 + g.kern as f32) * w.size * p.scale
                            / f.units_per_em as f32;
                        if used + adv > room(first) && (idx > 0 || !taken.is_empty()) {
                            break;
                        }
                        used += adv;
                        head.glyphs.push(*g);
                        head.text.push(g.ch);
                        idx += 1;
                    }
                    if idx == p.glyphs.len() {
                        taken.push(head);
                        continue;
                    }
                    p.glyphs.drain(..idx);
                    p.text = p.text.chars().skip(idx).collect();
                    taken.push(head);
                    current.push(Atom::Word(std::mem::take(&mut taken)));
                    finish(&mut current, &mut lines, &mut first, false);
                    used = 0.0;
                    rest.insert(0, p);
                }
                if !taken.is_empty() {
                    width = used;
                    current.push(Atom::Word(taken));
                }
            }
        }
    }
    if !current.is_empty() || lines.is_empty() {
        finish(&mut current, &mut lines, &mut first, true);
    }
    lines
}

// ── Pages ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageKind {
    Cover,
    /// Title, copyright, dedication, epigraph: no header, no number.
    Front,
    Contents,
    /// The first page of a chapter: no header, no number.
    Opening,
    Text,
    /// Left empty so a chapter can start on a right-hand page.
    Blank,
}

#[derive(Debug)]
struct Page {
    kind: PageKind,
    /// Distance from the top of the text block to the top of each line.
    lines: Vec<(f32, Line)>,
    /// The chapter whose title runs in the header, when there is one.
    chapter: Option<usize>,
    /// A chapter that begins here, for the bookmarks.
    starts: Option<usize>,
    used: f32,
}

impl Page {
    fn new(kind: PageKind, chapter: Option<usize>) -> Self {
        Page {
            kind,
            lines: Vec::new(),
            chapter,
            starts: None,
            used: 0.0,
        }
    }
}

struct Layout {
    pages: Vec<Page>,
    /// Height of the text block.
    capacity: f32,
    chapter_pages: Vec<usize>,
}

impl Layout {
    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a page is always open")
    }

    fn new_page(&mut self, kind: PageKind, chapter: Option<usize>) {
        self.pages.push(Page::new(kind, chapter));
    }

    fn at_top(&self) -> bool {
        self.pages
            .last()
            .is_none_or(|p| p.lines.is_empty() && p.used == 0.0)
    }

    /// White space, which a page top does not keep.
    fn gap(&mut self, points: f32) {
        if self.at_top() {
            return;
        }
        let capacity = self.capacity;
        let page = self.page();
        page.used = (page.used + points).min(capacity);
    }

    fn room(&self) -> f32 {
        self.pages.last().map_or(0.0, |p| self.capacity - p.used)
    }

    /// A paragraph's lines, kept off the foot and head of a page one line at a
    /// time: a lone first line is an orphan, a lone last line a widow.
    fn paragraph(&mut self, lines: Vec<Line>, kind: PageKind, chapter: Option<usize>) {
        let mut rest = lines;
        while !rest.is_empty() {
            let fit = {
                let room = self.room();
                let mut used = 0.0;
                let mut n = 0;
                for l in &rest {
                    if used + l.leading > room + 0.01 {
                        break;
                    }
                    used += l.leading;
                    n += 1;
                }
                n
            };
            let total = rest.len();
            let take = if fit >= total {
                total
            } else if total < 4 {
                // Too short to split without a widow or an orphan: move it whole.
                if self.at_top() {
                    fit.max(1)
                } else {
                    0
                }
            } else if fit >= 2 && total - fit >= 2 {
                fit
            } else if fit >= 2 && total - fit == 1 && fit >= 3 {
                fit - 1
            } else if self.at_top() {
                fit.max(1)
            } else {
                0
            };
            if take == 0 {
                self.new_page(kind, chapter);
                continue;
            }
            let placed: Vec<Line> = rest.drain(..take).collect();
            let page = self.page();
            for l in placed {
                let top = page.used;
                page.used += l.leading;
                page.lines.push((top, l));
            }
            if !rest.is_empty() {
                self.new_page(kind, chapter);
            }
        }
    }
}

// ── Geometry ────────────────────────────────────────────────────────────────

struct Geometry {
    width: f32,
    height: f32,
    outside: f32,
    inside: f32,
    top: f32,
    bottom: f32,
}

impl Geometry {
    fn from(options: &ExportOptions) -> Self {
        let pdf = options.pdf.clone().unwrap_or_default();
        let (mut w, mut h) = pdf.page_size.unwrap_or(PageSize::Letter).points();
        if pdf.landscape {
            std::mem::swap(&mut w, &mut h);
        }
        let margin = pdf.margin_in.unwrap_or(1.0).clamp(0.25, 3.0) * 72.0;
        // Facing pages bind on the inside edge, which wants a little more.
        let inside = pdf
            .inside_margin_in
            .map(|m| m.clamp(0.25, 3.0) * 72.0)
            .unwrap_or(margin * 1.2);
        Geometry {
            width: w,
            height: h,
            outside: margin,
            inside,
            top: margin,
            bottom: margin,
        }
    }
    fn measure(&self) -> f32 {
        self.width - self.outside - self.inside
    }
    fn capacity(&self) -> f32 {
        self.height - self.top - self.bottom
    }
    /// Where the text block starts: the inside margin is on the binding side,
    /// which is the left of a right-hand page.
    fn left(&self, physical: usize) -> f32 {
        if physical % 2 == 1 {
            self.inside
        } else {
            self.outside
        }
    }
}

/// The outside edge of a page: the left of a left-hand (even) page.
fn outside_edge(verso: bool) -> Place {
    if verso {
        Place::Left
    } else {
        Place::Right
    }
}

/// A page number as printed: its text, which side, and whether it goes in the
/// head line rather than the foot.
fn folio(pdf: &super::options::PdfOptions, label: &str, verso: bool) -> (String, Place, bool) {
    let text = match pdf.page_number_style {
        FolioStyle::Plain => label.to_string(),
        FolioStyle::Dashes => format!("– {label} –"),
    };
    let (place, top) = match pdf.page_number_position {
        FolioPosition::BottomCentre => (Place::Centre, false),
        FolioPosition::BottomOutside => (outside_edge(verso), false),
        FolioPosition::TopOutside => (outside_edge(verso), true),
    };
    (text, place, top)
}

// ── The document ────────────────────────────────────────────────────────────

fn roman(mut n: u32) -> String {
    const NUMERALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (value, text) in NUMERALS {
        while n >= value {
            out.push_str(text);
            n -= value;
        }
    }
    out
}

struct Plan<'a> {
    m: &'a Manuscript,
    r: ResolvedStyle,
    family: Family,
    geo: Geometry,
    place: Place,
    hyphenate: bool,
    pdf: super::options::PdfOptions,
}

impl Plan<'_> {
    fn leading(&self, size: f32) -> f32 {
        size * self.r.line_spacing.factor()
    }

    #[allow(clippy::too_many_arguments)]
    fn wrap(
        &self,
        atoms: Vec<Atom>,
        size: f32,
        first_indent: f32,
        indent: f32,
        measure: f32,
        place: Place,
        kind: Kind,
    ) -> Vec<Line> {
        wrap(
            atoms,
            &Wrap {
                size,
                leading: self.leading(size),
                family: self.family,
                first_indent,
                indent,
                measure,
                place,
                hyphenate: self.hyphenate,
                language: self.m.lang(),
                kind,
            },
        )
    }

    fn plain_lines(
        &self,
        text: &str,
        size: f32,
        bold: bool,
        italic: bool,
        place: Place,
        kind: Kind,
    ) -> Vec<Line> {
        let atoms = atoms_of(
            &[Span {
                text: text.to_string(),
                bold,
                italic,
                ..Default::default()
            }],
            &Ink {
                family: self.family,
                italic: false,
                bold: false,
            },
        );
        self.wrap(atoms, size, 0.0, 0.0, self.geo.measure(), place, kind)
    }
}

/// The scene-break mark, or `* * *` when the typeface has no glyph for it.
fn break_mark(plan: &Plan) -> String {
    let f = font(plan.family, Face::Regular);
    let mark = plan.r.scene_break.clone();
    if mark
        .chars()
        .all(|c| c.is_whitespace() || f.glyph(c).is_some())
    {
        mark
    } else {
        "* * *".to_string()
    }
}

fn lay_out(plan: &Plan) -> (Layout, Vec<String>) {
    let m = plan.m;
    let geo = &plan.geo;
    let size = plan.r.size_pt;
    let measure = geo.measure();
    let mut layout = Layout {
        pages: Vec::new(),
        capacity: geo.capacity(),
        chapter_pages: Vec::new(),
    };
    let indent_pts = plan.r.paragraph_indent_em * size;
    let para_gap = plan.r.paragraph_spacing_em * size;

    // The cover is the first page, and is not counted as a page of the book.
    if m.cover.is_some() {
        layout.new_page(PageKind::Cover, None);
    }

    // Front matter: each on a page of its own, a third of the way down.
    if !m.omit_front_matter {
        let centred = |text: &str, sz: f32, bold: bool, italic: bool| {
            plan.plain_lines(text, sz, bold, italic, Place::Centre, Kind::Head)
        };
        layout.new_page(PageKind::Front, None);
        layout.page().used = layout.capacity / 3.0;
        layout.paragraph(
            centred(&m.title, size * 2.4, true, false),
            PageKind::Front,
            None,
        );
        if let Some(subtitle) = &m.subtitle {
            layout.gap(size);
            layout.paragraph(
                centred(subtitle, size * 1.3, false, true),
                PageKind::Front,
                None,
            );
        }
        layout.gap(size * 2.0);
        layout.paragraph(
            centred(&format!("by {}", m.byline), size * 1.1, false, false),
            PageKind::Front,
            None,
        );
        if let Some(line) = &m.copyright_line {
            layout.new_page(PageKind::Front, None);
            layout.page().used = layout.capacity * 0.7;
            layout.paragraph(
                centred(line, size * 0.85, false, false),
                PageKind::Front,
                None,
            );
        }
        if let Some(dedication) = &m.dedication {
            layout.new_page(PageKind::Front, None);
            layout.page().used = layout.capacity / 3.0;
            layout.paragraph(
                centred(dedication, size, false, true),
                PageKind::Front,
                None,
            );
        }
        if let Some(epigraph) = &m.epigraph {
            layout.new_page(PageKind::Front, None);
            layout.page().used = layout.capacity / 3.0;
            layout.paragraph(centred(epigraph, size, false, true), PageKind::Front, None);
            if let Some(source) = &m.epigraph_source {
                layout.gap(size * 0.5);
                layout.paragraph(
                    centred(&format!("— {source}"), size * 0.9, false, false),
                    PageKind::Front,
                    None,
                );
            }
        }
    }

    // The contents page(s) are reserved now and filled once the chapters have
    // pages: how many it takes does not depend on what the numbers are.
    let toc_start = layout.pages.len();
    let mut toc_pages = 0;
    if m.toc && !m.chapters.is_empty() {
        let per_page = (layout.capacity / plan.leading(size)).floor() as usize - 3;
        toc_pages = m.chapters.len().div_ceil(per_page.max(1));
        for _ in 0..toc_pages {
            layout.new_page(PageKind::Contents, None);
        }
    }

    let chapter_family = plan.family;
    let _ = chapter_family;
    let right_hand = plan.pdf.chapters_on_right;
    let ink = Ink {
        family: plan.family,
        italic: false,
        bold: false,
    };
    let quote_ink = Ink {
        family: plan.family,
        italic: true,
        bold: false,
    };
    let note_size = size * 0.88;

    for (ci, chapter) in m.chapters.iter().enumerate() {
        // A chapter begins on a fresh page — a right-hand one if asked, which
        // sometimes means leaving the page before it blank.
        layout.new_page(PageKind::Opening, Some(ci));
        // Pages count from the first one after the cover; a right-hand page
        // has an odd position.
        let counted = |l: &Layout| l.pages.iter().filter(|p| p.kind != PageKind::Cover).count();
        if right_hand && counted(&layout) % 2 == 0 {
            let last = layout.pages.len() - 1;
            layout.pages[last] = Page::new(PageKind::Blank, None);
            layout.new_page(PageKind::Opening, Some(ci));
        }
        layout.page().starts = Some(ci);
        layout.chapter_pages.push(layout.pages.len() - 1);
        layout.page().used = layout.capacity
            / if chapter.role == super::Role::Part {
                3.0
            } else {
                6.0
            };

        let title = match (plan.r.heading_style, chapter.role) {
            (HeadingStyle::SmallCaps, _) => chapter.title.to_uppercase(),
            _ => chapter.title.clone(),
        };
        let title_size = size
            * if plan.r.heading_style == HeadingStyle::SmallCaps {
                1.5
            } else {
                1.8
            };
        layout.paragraph(
            plan.plain_lines(&title, title_size, true, false, Place::Centre, Kind::Head),
            PageKind::Opening,
            Some(ci),
        );
        layout.gap(size * 2.0);

        let mut after_break = true;
        for (si, scene) in chapter.scenes.iter().enumerate() {
            if si > 0 {
                layout.gap(plan.leading(size) * 0.5);
                let glyph = break_mark(plan);
                if !glyph.is_empty() {
                    layout.paragraph(
                        plan.plain_lines(&glyph, size, false, false, Place::Centre, Kind::Head),
                        PageKind::Text,
                        Some(ci),
                    );
                }
                layout.gap(plan.leading(size) * 0.5);
                after_break = true;
            }
            for block in &scene.blocks {
                match block {
                    Block::Paragraph(spans) => {
                        let atoms = atoms_of(spans, &ink);
                        if atoms.is_empty() {
                            continue;
                        }
                        let first = if after_break { 0.0 } else { indent_pts };
                        layout.gap(para_gap);
                        let lines =
                            plan.wrap(atoms, size, first, 0.0, measure, plan.place, Kind::Text);
                        layout.paragraph(lines, PageKind::Text, Some(ci));
                        after_break = false;
                    }
                    Block::Quote(spans) => {
                        let atoms = atoms_of(spans, &quote_ink);
                        layout.gap(plan.leading(size) * 0.4);
                        let inset = size * 2.0;
                        let lines = plan.wrap(
                            atoms,
                            size,
                            inset,
                            inset,
                            measure - inset,
                            plan.place,
                            Kind::Text,
                        );
                        // The block is indented on both sides: narrow the measure, shift the lines.
                        let lines = lines
                            .into_iter()
                            .map(|mut l| {
                                l.indent = inset;
                                l.measure = measure;
                                l.measure -= inset;
                                l
                            })
                            .collect();
                        layout.paragraph(lines, PageKind::Text, Some(ci));
                        layout.gap(plan.leading(size) * 0.4);
                        after_break = true;
                    }
                    Block::Heading { spans, .. } => {
                        let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                        layout.gap(plan.leading(size));
                        layout.paragraph(
                            plan.plain_lines(
                                &text,
                                size * 1.15,
                                true,
                                false,
                                Place::Centre,
                                Kind::Head,
                            ),
                            PageKind::Text,
                            Some(ci),
                        );
                        layout.gap(plan.leading(size) * 0.5);
                        after_break = true;
                    }
                    Block::SceneBreak => {
                        layout.gap(plan.leading(size) * 0.5);
                        let glyph = break_mark(plan);
                        if !glyph.is_empty() {
                            layout.paragraph(
                                plan.plain_lines(
                                    &glyph,
                                    size,
                                    false,
                                    false,
                                    Place::Centre,
                                    Kind::Head,
                                ),
                                PageKind::Text,
                                Some(ci),
                            );
                        }
                        layout.gap(plan.leading(size) * 0.5);
                        after_break = true;
                    }
                }
            }
        }

        // Notes collect at the end of the chapter: the face has no room
        // reserved at the foot of a page for them.
        let per_chapter = m.note_layout != NoteLayout::EndnotesBook;
        if per_chapter && !chapter.notes.is_empty() {
            layout.gap(plan.leading(size));
            layout.paragraph(
                plan.plain_lines("Notes", size, true, false, Place::Left, Kind::Head),
                PageKind::Text,
                Some(ci),
            );
            for note in &chapter.notes {
                let text = format!("{}  {}", note.number, note.plain(m.note_label));
                let atoms = atoms_of(
                    &[Span {
                        text,
                        ..Default::default()
                    }],
                    &ink,
                );
                let lines = plan.wrap(
                    atoms,
                    note_size,
                    0.0,
                    size * 1.5,
                    measure,
                    Place::Left,
                    Kind::Note,
                );
                layout.gap(note_size * 0.3);
                layout.paragraph(lines, PageKind::Text, Some(ci));
            }
        }
    }

    if m.note_layout == NoteLayout::EndnotesBook && m.chapters.iter().any(|c| !c.notes.is_empty()) {
        layout.new_page(PageKind::Opening, None);
        layout.page().used = layout.capacity / 6.0;
        layout.paragraph(
            plan.plain_lines("Notes", size * 1.8, true, false, Place::Centre, Kind::Head),
            PageKind::Opening,
            None,
        );
        layout.gap(size);
        for chapter in &m.chapters {
            if chapter.notes.is_empty() {
                continue;
            }
            layout.gap(plan.leading(size) * 0.5);
            layout.paragraph(
                plan.plain_lines(&chapter.title, size, true, false, Place::Left, Kind::Head),
                PageKind::Text,
                None,
            );
            for note in &chapter.notes {
                let text = format!("{}  {}", note.number, note.plain(m.note_label));
                let atoms = atoms_of(
                    &[Span {
                        text,
                        ..Default::default()
                    }],
                    &ink,
                );
                let lines = plan.wrap(
                    atoms,
                    note_size,
                    0.0,
                    size * 1.5,
                    measure,
                    Place::Left,
                    Kind::Note,
                );
                layout.gap(note_size * 0.3);
                layout.paragraph(lines, PageKind::Text, None);
            }
        }
    }

    // Number the pages, so the contents can be filled in.
    let labels = page_labels(&layout, plan);
    if toc_pages > 0 {
        fill_contents(&mut layout, plan, toc_start, toc_pages, &labels);
    }
    (layout, labels)
}

/// What each page is called: roman for the front matter when asked, arabic
/// after, from the first chapter. The cover is not a page of the book.
fn page_labels(layout: &Layout, plan: &Plan) -> Vec<String> {
    let first_body = layout
        .chapter_pages
        .first()
        .copied()
        .unwrap_or(layout.pages.len());
    let start = plan.pdf.start_page.unwrap_or(1).max(1);
    let mut labels = Vec::with_capacity(layout.pages.len());
    let mut front = 0u32;
    let mut body = 0u32;
    let mut continuous = 0u32;
    for (i, page) in layout.pages.iter().enumerate() {
        if page.kind == PageKind::Cover {
            labels.push(String::new());
            continue;
        }
        continuous += 1;
        if plan.pdf.roman_front_matter && i < first_body {
            front += 1;
            labels.push(roman(front));
        } else if plan.pdf.roman_front_matter {
            body += 1;
            labels.push((start + body - 1).to_string());
        } else {
            labels.push((start + continuous - 1).to_string());
        }
    }
    labels
}

fn fill_contents(layout: &mut Layout, plan: &Plan, first: usize, count: usize, labels: &[String]) {
    let m = plan.m;
    let size = plan.r.size_pt;
    let measure = plan.geo.measure();
    let family = plan.family;
    let per_page = (layout.capacity / plan.leading(size)).floor() as usize - 3;
    for k in 0..count {
        let index = first + k;
        let page = &mut layout.pages[index];
        page.used = 0.0;
        if k == 0 {
            let heading = plan.plain_lines(
                "Contents",
                size * 1.8,
                true,
                false,
                Place::Centre,
                Kind::Head,
            );
            for l in heading {
                page.lines.push((page.used, l.clone()));
                page.used += l.leading;
            }
            page.used += size * 2.0;
        }
        let slice = (k * per_page)..((k + 1) * per_page).min(m.chapters.len());
        for ci in slice {
            let title = &m.chapters[ci].title;
            let number = labels
                .get(layout.chapter_pages[ci])
                .cloned()
                .unwrap_or_default();
            // Title, a run of dots, and the page number flush right.
            let number_width: f32 = {
                let g = shape(&number, family, Face::Regular);
                let f = font(family, Face::Regular);
                g.iter().map(|g| g.advance as f32).sum::<f32>() * size / f.units_per_em as f32
            };
            let dot = {
                let f = font(family, Face::Regular);
                f.advance(f.glyph('.').unwrap_or(0)) as f32 * size / f.units_per_em as f32
            };
            let mut shown = title.clone();
            let max_title = measure - number_width - size * 2.0;
            while word_width(&[part(&shown, family, Face::Regular)], size) > max_title
                && shown.chars().count() > 4
            {
                shown.pop();
                shown = shown.trim_end().to_string();
                if !shown.ends_with('…') {
                    shown.push('…');
                    shown.pop();
                }
            }
            let used =
                word_width(&[part(&shown, family, Face::Regular)], size) + number_width + size;
            let dots = (((measure - used) / dot).floor() as usize).saturating_sub(1);
            let line_text = format!("{shown} {}", ".".repeat(dots));
            let atoms = vec![Atom::Word(vec![part(&line_text, family, Face::Regular)])];
            let line = Line {
                atoms,
                size,
                leading: plan.leading(size),
                indent: 0.0,
                measure,
                place: Place::Left,
                stretch: 0.0,
                family,
                kind: Kind::Contents,
            };
            // The number, set as its own line part by position: stored as a
            // second atom after a flexible space that fills to the margin.
            let mut line = line;
            let filler = measure
                - word_width(&[part(&line_text, family, Face::Regular)], size)
                - number_width;
            line.atoms.push(Atom::Space);
            line.stretch = filler.max(0.0);
            line.atoms
                .push(Atom::Word(vec![part(&number, family, Face::Regular)]));
            line.place = Place::Justified;
            page.lines.push((page.used, line));
            page.used += plan.leading(size);
        }
    }
}

// ── The file ────────────────────────────────────────────────────────────────

struct Pdf {
    objects: Vec<Vec<u8>>,
}

impl Pdf {
    fn reserve(&mut self) -> usize {
        self.objects.push(Vec::new());
        self.objects.len()
    }
    fn set(&mut self, id: usize, body: Vec<u8>) {
        self.objects[id - 1] = body;
    }
    fn add(&mut self, body: Vec<u8>) -> usize {
        let id = self.reserve();
        self.set(id, body);
        id
    }
    fn stream(&mut self, dict: &str, data: &[u8], compress: bool) -> usize {
        let (payload, filter) = if compress {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(data).expect("writing to memory");
            (
                enc.finish().expect("finishing in memory"),
                " /Filter /FlateDecode",
            )
        } else {
            (data.to_vec(), "")
        };
        let mut body =
            format!("<< {dict}{filter} /Length {} >>\nstream\n", payload.len()).into_bytes();
        body.extend_from_slice(&payload);
        body.extend_from_slice(b"\nendstream");
        self.add(body)
    }
}

/// A PDF text string: UTF-16 with a byte-order mark, in hex.
fn text_string(text: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in text.encode_utf16() {
        out.push_str(&format!("{unit:04X}"));
    }
    out.push('>');
    out
}

fn hex_glyphs(glyphs: &[u16]) -> String {
    glyphs.iter().map(|g| format!("{g:04X}")).collect()
}

struct Usage {
    /// Which fonts the document uses, and the glyphs of each.
    faces: BTreeMap<(Family, Face), BTreeMap<u16, char>>,
}

impl Usage {
    fn note(&mut self, part: &Part) {
        let entry = self.faces.entry((part.family, part.face)).or_default();
        for g in &part.glyphs {
            entry.entry(g.gid).or_insert(g.ch);
        }
    }
}

/// One line's drawing operators, positioned from the page's left edge.
fn draw_line(
    out: &mut String,
    line: &Line,
    left: f32,
    baseline: f32,
    usage: &mut Usage,
    fonts: &HashMap<(Family, Face), usize>,
) {
    let space = space_width(line.family, line.size);
    let spaces = line
        .atoms
        .iter()
        .filter(|a| matches!(a, Atom::Space))
        .count();
    let natural = natural_width(&line.atoms, line.size, line.family);
    let stretched = if line.kind == Kind::Contents {
        // One flexible space between the title and the number, whatever the count.
        natural + line.stretch
    } else {
        natural + line.stretch * spaces as f32
    };
    let mut x = left
        + match line.place {
            Place::Centre => (line.measure - stretched) / 2.0,
            Place::Right => line.measure - stretched,
            _ => line.indent,
        };
    for atom in &line.atoms {
        match atom {
            Atom::Space => x += space + line.stretch,
            Atom::Break => {}
            Atom::Word(parts) => {
                for p in parts {
                    usage.note(p);
                    let f = font(p.family, p.face);
                    let size = line.size * p.scale;
                    let id = fonts[&(p.family, p.face)];
                    // Kerning goes inside the string as a number: thousandths
                    // of the size, subtracted from the pen position.
                    let mut array = String::new();
                    let mut run: Vec<u16> = Vec::new();
                    for g in &p.glyphs {
                        if g.kern != 0 {
                            if !run.is_empty() {
                                array.push_str(&format!("<{}>", hex_glyphs(&run)));
                                run.clear();
                            }
                            array.push_str(&format!(
                                "{} ",
                                -(g.kern as f32) * 1000.0 / f.units_per_em as f32
                            ));
                        }
                        run.push(g.gid);
                    }
                    if !run.is_empty() {
                        array.push_str(&format!("<{}>", hex_glyphs(&run)));
                    }
                    out.push_str(&format!(
                        "BT /F{id} {size:.2} Tf 1 0 0 1 {x:.2} {:.2} Tm [{array}] TJ ET\n",
                        baseline + p.rise * line.size
                    ));
                    x += p.width(line.size);
                }
            }
        }
    }
}

pub struct Rendered {
    pub bytes: Vec<u8>,
    /// How many pages it came to, for the tests.
    #[allow(dead_code)]
    pub pages: usize,
}

pub fn write(
    manuscript: &Manuscript,
    options: &ExportOptions,
    max_pages: Option<usize>,
) -> Vec<u8> {
    render(manuscript, options, max_pages).bytes
}

pub fn render(m: &Manuscript, options: &ExportOptions, max_pages: Option<usize>) -> Rendered {
    let r = options.style.resolve();
    let pdf_options = options.pdf.clone().unwrap_or_default();
    let plan = Plan {
        m,
        family: Family::for_font(&r.font),
        geo: Geometry::from(options),
        place: match r.align {
            Align::Justify => Place::Justified,
            Align::Left => Place::Left,
        },
        hyphenate: r.hyphenate,
        pdf: pdf_options.clone(),
        r,
    };
    let (mut layout, mut labels) = lay_out(&plan);
    if let Some(limit) = max_pages {
        layout.pages.truncate(limit.max(1));
        labels.truncate(limit.max(1));
    }
    let geo = &plan.geo;
    let size = plan.r.size_pt;
    let show_numbers = pdf_options.page_numbers.unwrap_or(true);

    let mut pdf = Pdf {
        objects: Vec::new(),
    };
    let catalog = pdf.reserve();
    let pages_root = pdf.reserve();
    let info = pdf.reserve();

    // Fonts: ids are needed to draw, the files only once we know the glyphs.
    let mut fonts: HashMap<(Family, Face), usize> = HashMap::new();
    let mut font_objects: Vec<((Family, Face), usize)> = Vec::new();
    for face in Face::ALL {
        let id = pdf.reserve();
        fonts.insert((plan.family, face), id);
        font_objects.push(((plan.family, face), id));
    }
    let mut usage = Usage {
        faces: BTreeMap::new(),
    };

    // The cover, when there is one and it is a kind PDF can carry.
    let cover_image: Option<PdfImage> = m.cover.as_ref().and_then(|c| image::load(c).ok());
    let image_id = cover_image.as_ref().map(|_| pdf.reserve());
    let mask_id = cover_image
        .as_ref()
        .and_then(|i| i.alpha.as_ref())
        .map(|_| pdf.reserve());

    let page_ids: Vec<usize> = layout.pages.iter().map(|_| pdf.reserve()).collect();
    let mut chapter_dests: Vec<(usize, usize, f32)> = Vec::new(); // (chapter, page object, y)

    let mut physical = 0usize; // counted pages, excluding the cover
    for (index, page) in layout.pages.iter().enumerate() {
        let mut content = String::new();
        if page.kind != PageKind::Cover {
            physical += 1;
        }
        let left = geo.left(physical.max(1));
        let top = geo.height - geo.top;
        match page.kind {
            PageKind::Cover => {
                if let Some(img) = &cover_image {
                    // Fit the whole page, keeping the proportions.
                    let scale = (geo.width / img.width as f32).min(geo.height / img.height as f32);
                    let (w, h) = (img.width as f32 * scale, img.height as f32 * scale);
                    content.push_str(&format!(
                        "q {w:.2} 0 0 {h:.2} {:.2} {:.2} cm /Cover Do Q\n",
                        (geo.width - w) / 2.0,
                        (geo.height - h) / 2.0
                    ));
                }
            }
            _ => {
                for (offset, line) in &page.lines {
                    let baseline =
                        top - offset - (line.leading - line.size) / 2.0 - line.size * 0.78;
                    draw_line(&mut content, line, left, baseline, &mut usage, &fonts);
                }
                if let Some(ci) = page.starts {
                    chapter_dests.push((ci, page_ids[index], geo.height));
                }
                // Running header and folio, off the opening and front pages.
                if page.kind == PageKind::Text || page.kind == PageKind::Contents {
                    let small = size * 0.82;
                    let measure = geo.measure();
                    let head_y = geo.height - geo.top * 0.55;
                    let verso = physical.is_multiple_of(2);
                    let outside = outside_edge(verso);
                    let position = pdf_options.page_number_position;
                    // The running head sits on the outside edge, unless the
                    // page number has taken it.
                    let head_place = if show_numbers && position == FolioPosition::TopOutside {
                        Place::Centre
                    } else {
                        outside
                    };
                    let title = if page.kind == PageKind::Contents {
                        None
                    } else if verso {
                        Some((m.short_title.clone(), head_place))
                    } else {
                        page.chapter
                            .and_then(|c| m.chapters.get(c))
                            .map(|c| (c.title.clone(), head_place))
                    };
                    if let Some((text, place)) = title {
                        let atoms = atoms_of(
                            &[Span {
                                text,
                                italic: true,
                                ..Default::default()
                            }],
                            &Ink {
                                family: plan.family,
                                italic: false,
                                bold: false,
                            },
                        );
                        let line = Line {
                            atoms,
                            size: small,
                            leading: small,
                            indent: 0.0,
                            measure,
                            place,
                            stretch: 0.0,
                            family: plan.family,
                            kind: Kind::Head,
                        };
                        draw_line(&mut content, &line, left, head_y, &mut usage, &fonts);
                    }
                    if show_numbers && !labels[index].is_empty() {
                        let (text, place, top) = folio(&pdf_options, &labels[index], verso);
                        let y = if top { head_y } else { geo.bottom * 0.5 };
                        let atoms = atoms_of(
                            &[Span {
                                text,
                                ..Default::default()
                            }],
                            &Ink {
                                family: plan.family,
                                italic: false,
                                bold: false,
                            },
                        );
                        let line = Line {
                            atoms,
                            size: small,
                            leading: small,
                            indent: 0.0,
                            measure,
                            place,
                            stretch: 0.0,
                            family: plan.family,
                            kind: Kind::Head,
                        };
                        draw_line(&mut content, &line, left, y, &mut usage, &fonts);
                    }
                }
            }
        }

        let content_id = pdf.stream("", content.as_bytes(), true);
        let mut resources = String::from("<< /Font << ");
        for ((_, _), id) in &font_objects {
            resources.push_str(&format!("/F{id} {id} 0 R "));
        }
        resources.push_str(">>");
        if page.kind == PageKind::Cover {
            if let Some(id) = image_id {
                resources.push_str(&format!(" /XObject << /Cover {id} 0 R >>"));
            }
        }
        resources.push_str(" >>");
        pdf.set(
            page_ids[index],
            format!(
                "<< /Type /Page /Parent {pages_root} 0 R /MediaBox [0 0 {:.2} {:.2}] /Resources {resources} /Contents {content_id} 0 R >>",
                geo.width, geo.height
            )
            .into_bytes(),
        );
    }

    // The cover image and its alpha.
    if let (Some(img), Some(id)) = (&cover_image, image_id) {
        let mut dict = format!(
            "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent {} /Filter /{}",
            img.width, img.height, img.color_space, img.bits, img.filter
        );
        if let Some(parms) = &img.decode_parms {
            dict.push_str(&format!(" /DecodeParms {parms}"));
        }
        if let Some(mid) = mask_id {
            dict.push_str(&format!(" /SMask {mid} 0 R"));
        }
        let mut body = format!("<< {dict} /Length {} >>\nstream\n", img.data.len()).into_bytes();
        body.extend_from_slice(&img.data);
        body.extend_from_slice(b"\nendstream");
        pdf.set(id, body);
        if let (Some(alpha), Some(mid)) = (&img.alpha, mask_id) {
            let mut body = format!(
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n",
                alpha.width, alpha.height, alpha.data.len()
            )
            .into_bytes();
            body.extend_from_slice(&alpha.data);
            body.extend_from_slice(b"\nendstream");
            pdf.set(mid, body);
        }
    }

    // The fonts, subset to the glyphs the pages drew.
    for ((family, face), id) in &font_objects {
        let used = usage
            .faces
            .get(&(*family, *face))
            .cloned()
            .unwrap_or_default();
        write_font(
            &mut pdf,
            *id,
            font(*family, *face),
            &used,
            &tag_for(*family, *face),
        );
    }

    // Bookmarks, one per chapter.
    let outlines = if chapter_dests.is_empty() {
        None
    } else {
        let root = pdf.reserve();
        let items: Vec<usize> = chapter_dests.iter().map(|_| pdf.reserve()).collect();
        for (i, (ci, page_id, y)) in chapter_dests.iter().enumerate() {
            let title = &m.chapters[*ci].title;
            let mut body = format!(
                "<< /Title {} /Parent {root} 0 R /Dest [{page_id} 0 R /XYZ 0 {y:.2} null]",
                text_string(title)
            );
            if i > 0 {
                body.push_str(&format!(" /Prev {} 0 R", items[i - 1]));
            }
            if i + 1 < items.len() {
                body.push_str(&format!(" /Next {} 0 R", items[i + 1]));
            }
            body.push_str(" >>");
            pdf.set(items[i], body.into_bytes());
        }
        pdf.set(
            root,
            format!(
                "<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>",
                items[0],
                items[items.len() - 1],
                items.len()
            )
            .into_bytes(),
        );
        Some(root)
    };

    // Page labels: roman, then arabic from the first chapter.
    let mut nums = String::new();
    {
        let mut previous: Option<(bool, u32)> = None;
        let mut offset_counted = 0u32;
        for (i, page) in layout.pages.iter().enumerate() {
            if page.kind == PageKind::Cover {
                nums.push_str(&format!("{i} << /P (cover) >> "));
                continue;
            }
            offset_counted += 1;
            let label = &labels[i];
            let is_roman = label
                .chars()
                .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'));
            let value: u32 = if is_roman {
                offset_counted
            } else {
                label.parse().unwrap_or(offset_counted)
            };
            let continues = previous.is_some_and(|(r, v)| r == is_roman && v + 1 == value);
            if !continues {
                if is_roman {
                    nums.push_str(&format!("{i} << /S /r /St {value} >> "));
                } else {
                    nums.push_str(&format!("{i} << /S /D /St {value} >> "));
                }
            }
            previous = Some((is_roman, value));
        }
    }

    let kids: String = page_ids.iter().map(|id| format!("{id} 0 R ")).collect();
    pdf.set(
        pages_root,
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            page_ids.len()
        )
        .into_bytes(),
    );
    let mut catalog_body = format!(
        "<< /Type /Catalog /Pages {pages_root} 0 R /Lang ({}) /PageLabels << /Nums [{nums}] >>",
        m.lang().replace(['(', ')', '\\'], "")
    );
    if let Some(root) = outlines {
        catalog_body.push_str(&format!(" /Outlines {root} 0 R /PageMode /UseOutlines"));
    }
    catalog_body.push_str(" >>");
    pdf.set(catalog, catalog_body.into_bytes());

    let mut info_body = format!(
        "<< /Title {} /Author {} /Creator (Just Type Studio) /Producer (Just Type Studio)",
        text_string(&m.title),
        text_string(&m.byline)
    );
    if let Some(d) = &m.description {
        info_body.push_str(&format!(" /Subject {}", text_string(d)));
    }
    if !m.keywords.is_empty() {
        info_body.push_str(&format!(
            " /Keywords {}",
            text_string(&m.keywords.join(", "))
        ));
    }
    info_body.push_str(" >>");
    pdf.set(info, info_body.into_bytes());

    // Serialise.
    let mut out: Vec<u8> = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(pdf.objects.len());
    for (index, object) in pdf.objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", pdf.objects.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            pdf.objects.len() + 1
        )
        .as_bytes(),
    );
    Rendered {
        bytes: out,
        pages: layout.pages.len(),
    }
}

/// The six-letter prefix a subset font's name carries, so two subsets of one
/// face are not mistaken for each other.
fn tag_for(family: Family, face: Face) -> String {
    let n = (family as usize) * 4 + face as usize;
    let letters: String = (0..6)
        .map(|i| (b'A' + ((n * 7 + i * 3) % 26) as u8) as char)
        .collect();
    letters
}

fn write_font(pdf: &mut Pdf, id: usize, f: &Font, used: &BTreeMap<u16, char>, tag: &str) {
    let name = format!("{tag}+{}", f.postscript_name());
    let wanted: BTreeSet<u16> = used.keys().copied().collect();
    let subset = f.subset(&wanted);
    let file = {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&subset).expect("writing to memory");
        let payload = enc.finish().expect("finishing in memory");
        let mut body = format!(
            "<< /Length1 {} /Filter /FlateDecode /Length {} >>\nstream\n",
            subset.len(),
            payload.len()
        )
        .into_bytes();
        body.extend_from_slice(&payload);
        body.extend_from_slice(b"\nendstream");
        pdf.add(body)
    };

    let scale = 1000.0 / f.units_per_em as f32;
    let mut flags = 32; // nonsymbolic
    if f.family == Family::Serif {
        flags |= 2;
    }
    if matches!(f.face, Face::Italic | Face::BoldItalic) {
        flags |= 64;
    }
    let descriptor = pdf.add(
        format!(
            "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {:.1} /Ascent {} /Descent {} /CapHeight {} /StemV {} /FontFile2 {file} 0 R >>",
            (f.bbox[0] as f32 * scale) as i32,
            (f.bbox[1] as f32 * scale) as i32,
            (f.bbox[2] as f32 * scale) as i32,
            (f.bbox[3] as f32 * scale) as i32,
            f.italic_angle,
            (f.ascent as f32 * scale) as i32,
            (f.descent as f32 * scale) as i32,
            if f.cap_height > 0 { (f.cap_height as f32 * scale) as i32 } else { 700 },
            if matches!(f.face, Face::Bold | Face::BoldItalic) { 140 } else { 80 },
        )
        .into_bytes(),
    );

    let widths: String = used
        .keys()
        .map(|gid| {
            format!(
                "{gid} [{}] ",
                (f.advance(*gid) as f32 * scale).round() as i32
            )
        })
        .collect();
    let descendant = pdf.add(
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /CIDToGIDMap /Identity /DW 500 /W [{widths}] >>"
        )
        .into_bytes(),
    );

    // So the text can be searched and copied: each glyph's character.
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def /CMapType 2 def\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = used.iter().collect();
    for chunk in entries.chunks(100) {
        cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, ch) in chunk {
            let mut units = [0u16; 2];
            let encoded = ch.encode_utf16(&mut units);
            let hex: String = encoded.iter().map(|u| format!("{u:04X}")).collect();
            cmap.push_str(&format!("<{gid:04X}> <{hex}>\n"));
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
    let to_unicode = pdf.stream("", cmap.as_bytes(), true);

    pdf.set(
        id,
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{descendant} 0 R] /ToUnicode {to_unicode} 0 R >>"
        )
        .into_bytes(),
    );
}

/// Things worth telling the writer before the file goes out.
pub fn warnings(m: &Manuscript, options: &ExportOptions) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let family = Family::for_font(&options.style.resolve().font);
    let f = font(family, Face::Regular);
    let mut missing: BTreeSet<char> = BTreeSet::new();
    for chapter in &m.chapters {
        for scene in &chapter.scenes {
            for block in &scene.blocks {
                let spans = match block {
                    Block::Paragraph(s) | Block::Quote(s) => s,
                    Block::Heading { spans, .. } => spans,
                    Block::SceneBreak => continue,
                };
                for span in spans {
                    for ch in span.text.chars() {
                        if !ch.is_whitespace() && f.glyph(ch).is_none() {
                            missing.insert(ch);
                        }
                    }
                }
            }
        }
    }
    if !missing.is_empty() {
        let shown: String = missing.iter().take(8).collect();
        out.push((
            "pdf_missing_glyphs",
            format!(
                "The PDF typeface has no glyph for {} character{} ({shown}), which will print as “?”. \
                 Use Word or EPUB for text in other scripts.",
                missing.len(),
                if missing.len() == 1 { "" } else { "s" }
            ),
        ));
    }
    if m.rtl {
        out.push((
            "pdf_rtl",
            "PDF book layout sets text left to right. Use EPUB or HTML for a right-to-left language.".into(),
        ));
    }
    if let Some(cover) = &m.cover {
        if let Err(why) = image::load(cover) {
            out.push((
                "pdf_cover_unsupported",
                format!("The cover cannot go in the PDF ({why}), so the PDF has no cover page."),
            ));
        }
    }
    if options.style.resolve().drop_caps {
        out.push(("pdf_drop_caps", "Drop caps are not applied in PDF.".into()));
    }
    if options.style.resolve().hyphenate && !can_hyphenate(m.lang()) {
        out.push((
            "pdf_no_hyphenation",
            format!(
                "There are no hyphenation patterns for the language “{}”, so the PDF is not \
                 hyphenated. Check the language under Book → Front matter.",
                m.lang()
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::options::{Format, PdfOptions, StyleOptions, StylePreset};
    use crate::export::Role;
    use crate::export::{ChapterContent, Cover, Note, SceneContent};

    fn lorem(words: usize) -> String {
        const W: [&str; 12] = [
            "The", "field", "went", "on", "past", "where", "the", "light", "gave", "out,", "and",
            "she",
        ];
        (0..words)
            .map(|i| W[i % W.len()])
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn book(chapters: usize, words: usize) -> Manuscript {
        Manuscript {
            title: "The Long Field".into(),
            subtitle: Some("A Novel".into()),
            byline: "Ada Quill".into(),
            surname: "Quill".into(),
            short_title: "THE LONG FIELD".into(),
            language: "en".into(),
            chapters: (1..=chapters)
                .map(|n| ChapterContent {
                    id: format!("c{n}"),
                    title: format!("Chapter {n}"),
                    role: Role::Chapter,
                    scenes: vec![
                        SceneContent {
                            blocks: (0..8)
                                .map(|_| {
                                    Block::Paragraph(vec![Span {
                                        text: lorem(words),
                                        ..Default::default()
                                    }])
                                })
                                .collect(),
                            ..Default::default()
                        },
                        SceneContent {
                            blocks: vec![Block::Paragraph(vec![Span {
                                text: lorem(words / 2),
                                italic: true,
                                ..Default::default()
                            }])],
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    fn options() -> ExportOptions {
        ExportOptions {
            format: Format::Pdf,
            style: StyleOptions {
                preset: StylePreset::Book,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn plan_of<'a>(m: &'a Manuscript, o: &ExportOptions) -> Plan<'a> {
        let r = o.style.resolve();
        Plan {
            m,
            family: Family::for_font(&r.font),
            geo: Geometry::from(o),
            place: if r.align == Align::Justify {
                Place::Justified
            } else {
                Place::Left
            },
            hyphenate: r.hyphenate,
            pdf: o.pdf.clone().unwrap_or_default(),
            r,
        }
    }

    #[test]
    fn it_writes_a_pdf_with_embedded_fonts_and_text() {
        let m = book(2, 90);
        let out = render(&m, &options(), None);
        assert!(out.bytes.starts_with(b"%PDF-1.5"));
        assert!(out.bytes.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&out.bytes);
        assert!(text.contains("/FontFile2"));
        assert!(text.contains("/Subtype /Type0"));
        assert!(text.contains("/Outlines"));
        assert!(text.contains("/PageLabels"));
        assert!(out.pages >= 5);
        // Output is deterministic.
        assert_eq!(out.bytes, render(&m, &options(), None).bytes);
    }

    #[test]
    fn no_line_is_wider_than_its_measure() {
        let m = book(3, 120);
        let o = options();
        let plan = plan_of(&m, &o);
        let (layout, _) = lay_out(&plan);
        let measure = plan.geo.measure();
        for page in &layout.pages {
            for (_, line) in &page.lines {
                let width = natural_width(&line.atoms, line.size, line.family);
                assert!(
                    width + line.indent <= measure + 0.5,
                    "a line of {width} + {} runs past {measure}",
                    line.indent
                );
            }
        }
    }

    #[test]
    fn justified_lines_fill_the_measure() {
        let m = book(1, 200);
        let o = options();
        let plan = plan_of(&m, &o);
        let (layout, _) = lay_out(&plan);
        let measure = plan.geo.measure();
        let mut justified = 0;
        for page in &layout.pages {
            for (_, line) in page
                .lines
                .iter()
                .filter(|(_, l)| l.kind == Kind::Text && l.place == Place::Justified)
            {
                let spaces = line
                    .atoms
                    .iter()
                    .filter(|a| matches!(a, Atom::Space))
                    .count();
                let w = natural_width(&line.atoms, line.size, line.family)
                    + line.stretch * spaces as f32;
                assert!(
                    (w + line.indent - measure).abs() < 0.5,
                    "{w} + {} vs {measure}",
                    line.indent
                );
                justified += 1;
            }
        }
        assert!(justified > 10);
    }

    #[test]
    fn chapters_open_on_new_pages_and_on_the_right_when_asked() {
        let m = book(3, 60);
        let mut o = options();
        o.pdf = Some(PdfOptions {
            chapters_on_right: true,
            ..Default::default()
        });
        let plan = plan_of(&m, &o);
        let (layout, _) = lay_out(&plan);
        assert_eq!(layout.chapter_pages.len(), 3);
        for &p in &layout.chapter_pages {
            let counted = layout.pages[..=p]
                .iter()
                .filter(|x| x.kind != PageKind::Cover)
                .count();
            assert_eq!(counted % 2, 1, "chapter opens on a left-hand page");
            assert_eq!(layout.pages[p].kind, PageKind::Opening);
        }
    }

    #[test]
    fn no_page_starts_or_ends_with_a_widow_or_an_orphan() {
        let m = book(2, 140);
        let o = options();
        let plan = plan_of(&m, &o);
        let (layout, _) = lay_out(&plan);
        // Paragraphs are kept together by the flow; check the observable
        // consequence: a text page never ends on the first line of a
        // paragraph of four or more lines whose rest is on the next page.
        for pair in layout.pages.windows(2) {
            let last = pair[0].lines.last();
            let next_first = pair[1].lines.first();
            if let (Some((_, a)), Some((_, b))) = (last, next_first) {
                if a.kind == Kind::Text && b.kind == Kind::Text && pair[1].kind == PageKind::Text {
                    // A paragraph's first line is indented; its rest is not.
                    let a_is_first = a.indent > 0.0;
                    let b_is_first = b.indent > 0.0;
                    assert!(
                        !a_is_first || b_is_first,
                        "an orphan was left at the foot of a page"
                    );
                }
            }
        }
    }

    #[test]
    fn the_contents_page_has_the_real_page_numbers() {
        let mut m = book(3, 90);
        m.toc = true;
        let mut o = options();
        o.pdf = Some(PdfOptions {
            roman_front_matter: true,
            ..Default::default()
        });
        let plan = plan_of(&m, &o);
        let (layout, labels) = lay_out(&plan);
        let toc = layout
            .pages
            .iter()
            .find(|p| p.kind == PageKind::Contents)
            .unwrap();
        let entries: Vec<String> = toc
            .lines
            .iter()
            .filter(|(_, l)| l.kind == Kind::Contents)
            .map(|(_, l)| {
                l.atoms
                    .iter()
                    .rev()
                    .find_map(|a| match a {
                        Atom::Word(p) => Some(p.iter().map(|x| x.text.clone()).collect::<String>()),
                        _ => None,
                    })
                    .unwrap()
            })
            .collect();
        assert_eq!(entries.len(), 3);
        for (ci, shown) in entries.iter().enumerate() {
            assert_eq!(shown, &labels[layout.chapter_pages[ci]]);
        }
        // Front matter is roman, the first chapter is page 1.
        assert_eq!(labels[layout.chapter_pages[0]], "1");
        assert!(labels[0] == "i");
    }

    #[test]
    fn page_size_margins_and_landscape_change_the_page() {
        let m = book(1, 40);
        let mut o = options();
        o.pdf = Some(PdfOptions {
            page_size: Some(PageSize::Trade6x9),
            margin_in: Some(0.75),
            ..Default::default()
        });
        let text = String::from_utf8_lossy(&render(&m, &o, None).bytes).to_string();
        assert!(text.contains("/MediaBox [0 0 432.00 648.00]"));
        o.pdf = Some(PdfOptions {
            page_size: Some(PageSize::A4),
            landscape: true,
            ..Default::default()
        });
        let text = String::from_utf8_lossy(&render(&m, &o, None).bytes).to_string();
        assert!(text.contains("/MediaBox [0 0 842.00 595.00]"));
    }

    #[test]
    fn page_numbers_go_where_and_as_asked() {
        let mut pdf = PdfOptions::default();
        assert_eq!(folio(&pdf, "7", false), ("7".into(), Place::Centre, false));
        pdf.page_number_position = FolioPosition::BottomOutside;
        pdf.page_number_style = FolioStyle::Dashes;
        assert_eq!(folio(&pdf, "8", true), ("– 8 –".into(), Place::Left, false));
        assert_eq!(folio(&pdf, "9", false).1, Place::Right);
        pdf.page_number_position = FolioPosition::TopOutside;
        assert_eq!(
            folio(&pdf, "xii", true),
            ("– xii –".into(), Place::Left, true)
        );

        // The dashes are in the bundled faces, so they print, not as "?".
        let m = book(2, 120);
        let mut o = options();
        o.pdf = Some(pdf);
        assert!(warnings(&m, &o).is_empty());
        assert!(render(&m, &o, None).pages > 2);
    }

    #[test]
    fn the_gutter_widens_the_inside_margin_only() {
        let mut o = options();
        let plain = Geometry::from(&o);
        assert!(plain.inside > plain.outside);
        o.pdf = Some(PdfOptions {
            margin_in: Some(0.5),
            inside_margin_in: Some(1.0),
            ..Default::default()
        });
        let g = Geometry::from(&o);
        assert_eq!((g.outside, g.inside), (36.0, 72.0));
        assert_eq!(g.left(1), 72.0); // a right-hand page binds on its left
        assert_eq!(g.left(2), 36.0);
        o.pdf.as_mut().unwrap().inside_margin_in = Some(9.0);
        assert!(o.validate().unwrap_err().contains("insideMarginIn"));
    }

    #[test]
    fn a_cover_is_the_first_page_and_not_numbered() {
        let mut m = book(1, 40);
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        // A valid 1x1 RGB PNG whose data is not decoded by the writer.
        let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&ihdr);
        png.extend_from_slice(&[0, 0, 0, 0]);
        let idat = [0x78, 0x9c, 0x63, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01];
        png.extend_from_slice(&(idat.len() as u32).to_be_bytes());
        png.extend_from_slice(b"IDAT");
        png.extend_from_slice(&idat);
        png.extend_from_slice(&[0, 0, 0, 0]);
        m.cover = Some(Cover {
            bytes: png,
            mime: "image/png".into(),
        });
        let o = options();
        let plan = plan_of(&m, &o);
        let (layout, labels) = lay_out(&plan);
        assert_eq!(layout.pages[0].kind, PageKind::Cover);
        assert_eq!(labels[0], "");
        assert_eq!(labels[1], "1");
        let text = String::from_utf8_lossy(&render(&m, &o, None).bytes).to_string();
        assert!(text.contains("/Cover Do") || text.contains("/Subtype /Image"));
    }

    #[test]
    fn notes_get_marks_and_a_list_after_the_chapter() {
        let mut m = book(1, 30);
        m.note_layout = NoteLayout::EndnotesChapter;
        m.chapters[0].scenes[0].blocks[0] = Block::Paragraph(vec![
            Span {
                text: "Hello".into(),
                ..Default::default()
            },
            Span {
                note: Some(1),
                ..Default::default()
            },
        ]);
        m.chapters[0].notes = vec![Note {
            number: 1,
            text: "the note".into(),
            quote: None,
        }];
        let o = options();
        let plan = plan_of(&m, &o);
        let (layout, _) = lay_out(&plan);
        let all: Vec<&Line> = layout
            .pages
            .iter()
            .flat_map(|p| p.lines.iter().map(|(_, l)| l))
            .collect();
        assert!(all.iter().any(|l| l.kind == Kind::Note));
    }

    #[test]
    fn a_fifty_page_book_is_quick() {
        let m = book(25, 160);
        let start = std::time::Instant::now();
        let out = render(&m, &options(), None);
        assert!(out.pages > 50, "{}", out.pages);
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    }

    /// A 100k-word book, hyphenated, in under ten seconds. That bar is for the
    /// release build in the Docker image; a debug test build clears it too.
    #[test]
    fn a_novel_lays_out_in_under_ten_seconds() {
        // Long words, so the hyphenator has work to do on most lines.
        const W: [&str; 10] = [
            "Nevertheless",
            "the",
            "extraordinary",
            "conversation",
            "continued",
            "underneath",
            "a",
            "remarkably",
            "unseasonable",
            "afternoon",
        ];
        let mut m = book(40, 1);
        let paragraph = (0..250)
            .map(|i| W[i % W.len()])
            .collect::<Vec<_>>()
            .join(" ");
        for chapter in &mut m.chapters {
            for scene in &mut chapter.scenes {
                scene.blocks = (0..5)
                    .map(|_| {
                        Block::Paragraph(vec![Span {
                            text: paragraph.clone(),
                            ..Default::default()
                        }])
                    })
                    .collect();
            }
        }
        let words: usize = m
            .chapters
            .iter()
            .flat_map(|c| &c.scenes)
            .map(|s| s.blocks.len() * 250)
            .sum();
        assert!(words >= 100_000, "{words}");
        let mut o = options();
        o.style.hyphenate = Some(true);
        o.content.toc = Some(true);
        let start = std::time::Instant::now();
        let out = render(&m, &o, None);
        let took = start.elapsed();
        eprintln!("{} pages in {took:?}", out.pages);
        assert!(took.as_secs_f32() < 10.0, "{took:?}");
    }

    #[test]
    fn hyphenation_breaks_where_a_typesetter_would() {
        assert_eq!(hyphen_points("hyphenation", "en"), vec![2, 6, 7]); // hy-phen-a-tion
        assert_eq!(hyphen_points("Mountain", "en-US"), vec![4]); // Moun-tain
        assert!(hyphen_points("cat", "en").is_empty());
        assert!(hyphen_points("it's-here", "en").is_empty());
        // Whatever it finds leaves enough on each side.
        for w in ["information", "extraordinary", "conversation", "Wednesday"] {
            for p in hyphen_points(w, "en") {
                assert!(p >= 2 && w.chars().count() - p >= 2, "{w} at {p}");
            }
        }
    }

    #[test]
    fn hyphenation_follows_the_books_language() {
        // The same letters break differently by language, and a language
        // without patterns is left alone rather than broken as English.
        assert!(!hyphen_points("Geschwindigkeit", "de").is_empty());
        assert!(!hyphen_points("anticonstitutionnellement", "fr-CA").is_empty());
        assert!(!hyphen_points("достопримечательность", "ru").is_empty());
        assert!(can_hyphenate("en-GB") && can_hyphenate("pt-BR") && can_hyphenate("nb"));
        assert!(!can_hyphenate("ja") && !can_hyphenate("xx"));
        assert!(hyphen_points("information", "ja").is_empty());
        // Indices are characters, not bytes, so accented words cut cleanly.
        for p in hyphen_points("développement", "fr") {
            assert!(p < "développement".chars().count());
        }
    }

    #[test]
    fn hyphenation_makes_lines_fuller_not_wider() {
        let m = book(1, 120);
        let mut o = options();
        o.style.hyphenate = Some(true);
        let plan = plan_of(&m, &o);
        let (layout, _) = lay_out(&plan);
        let measure = plan.geo.measure();
        for page in &layout.pages {
            for (_, line) in &page.lines {
                assert!(
                    natural_width(&line.atoms, line.size, line.family) + line.indent
                        <= measure + 0.5
                );
            }
        }
    }

    #[test]
    fn a_missing_glyph_is_reported_before_export() {
        let mut m = book(1, 10);
        m.chapters[0].scenes[0].blocks[0] = Block::Paragraph(vec![Span {
            text: "漢字 text".into(),
            ..Default::default()
        }]);
        let w = warnings(&m, &options());
        assert!(w.iter().any(|(code, _)| *code == "pdf_missing_glyphs"));
    }

    #[test]
    fn roman_numerals() {
        assert_eq!(roman(1), "i");
        assert_eq!(roman(4), "iv");
        assert_eq!(roman(9), "ix");
        assert_eq!(roman(14), "xiv");
        assert_eq!(roman(1994), "mcmxciv");
    }

    /// Real files for a human, and poppler, to open.
    #[test]
    #[ignore]
    fn dump_samples() {
        let dir = std::env::var("DUMP_DIR").unwrap();
        let mut m = book(4, 110);
        m.toc = true;
        m.dedication = Some("For Sam".into());
        m.epigraph = Some("Whatever comes, comes.".into());
        m.epigraph_source = Some("Anon".into());
        m.copyright_line = Some("© 2026 Ada Quill".into());
        m.note_layout = NoteLayout::EndnotesChapter;
        m.chapters[0].scenes[0].blocks[0] = Block::Paragraph(vec![
            Span {
                text: "The field went on".into(),
                ..Default::default()
            },
            Span {
                note: Some(1),
                ..Default::default()
            },
            Span {
                text:
                    " past where the light gave out, with “quotes”, — dashes and an AVATAR of Wave."
                        .into(),
                ..Default::default()
            },
        ]);
        m.chapters[0].notes = vec![Note {
            number: 1,
            text: "Show, do not tell.".into(),
            quote: Some("The field went on".into()),
        }];
        if let Ok(png) = std::fs::read(format!("{dir}/cover.png")) {
            m.cover = Some(Cover {
                bytes: png,
                mime: "image/png".into(),
            });
        }
        let mut o = options();
        o.pdf = Some(PdfOptions {
            roman_front_matter: true,
            chapters_on_right: true,
            page_size: Some(PageSize::Trade6x9),
            margin_in: Some(0.75),
            ..Default::default()
        });
        o.style.hyphenate = Some(true);
        o.style.size_pt = Some(11.0);
        std::fs::write(format!("{dir}/book.pdf"), write(&m, &o, None)).unwrap();
        let o2 = options();
        std::fs::write(format!("{dir}/letter.pdf"), write(&book(2, 80), &o2, None)).unwrap();
    }
}
