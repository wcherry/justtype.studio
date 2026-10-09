//! Reading just enough of a TrueType font to set type with it and to embed it.
//!
//! Book-layout PDF needs the real width of every glyph (to break lines and
//! justify them), the pairs the font kerns, and a way to ship the glyphs a book
//! actually uses inside the file, so it looks the same on a machine that has
//! never heard of the typeface. That is four small jobs — parse `cmap`,
//! `hmtx` and `kern`, and cut a subset out of `glyf` — and doing them here
//! costs less than a dependency that does a hundred other things.
//!
//! The bundled faces are Liberation Serif and Liberation Sans, under the SIL
//! Open Font License (see `assets/fonts/LICENSE.txt`).
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

/// Which of a family's four faces a run of text is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Face {
    Regular,
    Italic,
    Bold,
    BoldItalic,
}

impl Face {
    pub const ALL: [Face; 4] = [Face::Regular, Face::Italic, Face::Bold, Face::BoldItalic];

    pub fn of(bold: bool, italic: bool) -> Face {
        match (bold, italic) {
            (false, false) => Face::Regular,
            (false, true) => Face::Italic,
            (true, false) => Face::Bold,
            (true, true) => Face::BoldItalic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Family {
    Serif,
    Sans,
}

impl Family {
    /// The family a style's font choice is set in. The curated list names
    /// faces a reader's machine may have; the PDF carries its own, so the
    /// serif choices share one serif and the sans choices one sans.
    pub fn for_font(name: &str) -> Family {
        match name {
            "Helvetica" | "Verdana" => Family::Sans,
            _ => Family::Serif,
        }
    }

    fn base_name(self) -> &'static str {
        match self {
            Family::Serif => "LiberationSerif",
            Family::Sans => "LiberationSans",
        }
    }
}

macro_rules! bundled {
    ($file:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/fonts/", $file)) as &[u8]
    };
}

fn bytes_of(family: Family, face: Face) -> &'static [u8] {
    match (family, face) {
        (Family::Serif, Face::Regular) => bundled!("LiberationSerif-Regular.ttf"),
        (Family::Serif, Face::Italic) => bundled!("LiberationSerif-Italic.ttf"),
        (Family::Serif, Face::Bold) => bundled!("LiberationSerif-Bold.ttf"),
        (Family::Serif, Face::BoldItalic) => bundled!("LiberationSerif-BoldItalic.ttf"),
        (Family::Sans, Face::Regular) => bundled!("LiberationSans-Regular.ttf"),
        (Family::Sans, Face::Italic) => bundled!("LiberationSans-Italic.ttf"),
        (Family::Sans, Face::Bold) => bundled!("LiberationSans-Bold.ttf"),
        (Family::Sans, Face::BoldItalic) => bundled!("LiberationSans-BoldItalic.ttf"),
    }
}

/// A parsed face. Parsing happens once per process and the result is shared.
pub struct Font {
    data: &'static [u8],
    pub family: Family,
    pub face: Face,
    pub units_per_em: u16,
    pub ascent: i16,
    pub descent: i16,
    pub bbox: [i16; 4],
    pub italic_angle: f32,
    pub cap_height: i16,
    num_glyphs: u16,
    num_h_metrics: u16,
    tables: HashMap<[u8; 4], (usize, usize)>,
    cmap: HashMap<u32, u16>,
    kern: HashMap<(u16, u16), i16>,
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    d.get(at..at + 2)
        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]))
}
fn i16_at(d: &[u8], at: usize) -> i16 {
    u16_at(d, at) as i16
}
fn u32_at(d: &[u8], at: usize) -> u32 {
    d.get(at..at + 4)
        .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Font {
    fn parse(data: &'static [u8], family: Family, face: Face) -> Font {
        let count = u16_at(data, 4) as usize;
        let mut tables = HashMap::new();
        for i in 0..count {
            let at = 12 + i * 16;
            let tag: [u8; 4] = data[at..at + 4]
                .try_into()
                .expect("a table tag is four bytes");
            tables.insert(
                tag,
                (
                    u32_at(data, at + 8) as usize,
                    u32_at(data, at + 12) as usize,
                ),
            );
        }
        let table = |tag: &[u8; 4]| tables.get(tag).copied().unwrap_or((0, 0));

        let (head, _) = table(b"head");
        let (hhea, _) = table(b"hhea");
        let (maxp, _) = table(b"maxp");
        let (post, _) = table(b"post");
        let (os2, os2_len) = table(b"OS/2");

        let mut font = Font {
            data,
            family,
            face,
            units_per_em: u16_at(data, head + 18),
            ascent: i16_at(data, hhea + 4),
            descent: i16_at(data, hhea + 6),
            bbox: [
                i16_at(data, head + 36),
                i16_at(data, head + 38),
                i16_at(data, head + 40),
                i16_at(data, head + 42),
            ],
            // Fixed 16.16, at post+4.
            italic_angle: u32_at(data, post + 4) as i32 as f32 / 65536.0,
            cap_height: if os2_len >= 90 {
                i16_at(data, os2 + 88)
            } else {
                0
            },
            num_glyphs: u16_at(data, maxp + 4),
            num_h_metrics: u16_at(data, hhea + 34),
            tables,
            cmap: HashMap::new(),
            kern: HashMap::new(),
        };
        font.cmap = font.read_cmap();
        font.kern = font.read_kern();
        font
    }

    fn table(&self, tag: &[u8; 4]) -> &'static [u8] {
        match self.tables.get(tag) {
            Some(&(off, len)) => self.data.get(off..off + len).unwrap_or(&[]),
            None => &[],
        }
    }

    fn read_cmap(&self) -> HashMap<u32, u16> {
        let cmap = self.table(b"cmap");
        let mut best: Option<(u8, usize)> = None; // (rank, offset)
        for i in 0..u16_at(cmap, 2) as usize {
            let rec = 4 + i * 8;
            let (platform, encoding) = (u16_at(cmap, rec), u16_at(cmap, rec + 2));
            let offset = u32_at(cmap, rec + 4) as usize;
            let rank = match (platform, encoding) {
                (3, 10) => 3,
                (0, 4) | (0, 6) => 3,
                (3, 1) => 2,
                (0, _) => 1,
                _ => 0,
            };
            if rank > 0 && best.is_none_or(|(r, _)| rank > r) {
                best = Some((rank, offset));
            }
        }
        let mut map = HashMap::new();
        let Some((_, off)) = best else { return map };
        let sub = &cmap[off.min(cmap.len())..];
        match u16_at(sub, 0) {
            4 => {
                let segs = u16_at(sub, 6) as usize / 2;
                let end = 14;
                let start = end + segs * 2 + 2;
                let delta = start + segs * 2;
                let range = delta + segs * 2;
                for s in 0..segs {
                    let (e, b) = (
                        u16_at(sub, end + s * 2) as u32,
                        u16_at(sub, start + s * 2) as u32,
                    );
                    let d = u16_at(sub, delta + s * 2);
                    let ro = u16_at(sub, range + s * 2) as usize;
                    for c in b..=e {
                        if c == 0xFFFF {
                            continue;
                        }
                        let gid = if ro == 0 {
                            (c as u16).wrapping_add(d)
                        } else {
                            let at = range + s * 2 + ro + (c - b) as usize * 2;
                            match u16_at(sub, at) {
                                0 => 0,
                                g => g.wrapping_add(d),
                            }
                        };
                        if gid != 0 {
                            map.insert(c, gid);
                        }
                    }
                }
            }
            12 => {
                for g in 0..u32_at(sub, 12) as usize {
                    let at = 16 + g * 12;
                    let (b, e, first) = (u32_at(sub, at), u32_at(sub, at + 4), u32_at(sub, at + 8));
                    for c in b..=e.min(b + 0x10000) {
                        let gid = first + (c - b);
                        if gid != 0 && gid <= u16::MAX as u32 {
                            map.insert(c, gid as u16);
                        }
                    }
                }
            }
            _ => {}
        }
        map
    }

    /// The legacy `kern` table, format 0 — pairs of glyphs and how far to move
    /// the second. (Newer fonts keep kerning in GPOS, which is a far larger
    /// job; these faces carry both.)
    fn read_kern(&self) -> HashMap<(u16, u16), i16> {
        let kern = self.table(b"kern");
        let mut pairs = HashMap::new();
        let mut at = 4;
        for _ in 0..u16_at(kern, 2) {
            let len = u16_at(kern, at + 2) as usize;
            let coverage = u16_at(kern, at + 4);
            // Format 0 (high byte), horizontal (bit 0).
            if coverage >> 8 == 0 && coverage & 1 == 1 {
                for p in 0..u16_at(kern, at + 6) as usize {
                    let r = at + 14 + p * 6;
                    pairs.insert((u16_at(kern, r), u16_at(kern, r + 2)), i16_at(kern, r + 4));
                }
            }
            at += len.max(6);
        }
        pairs
    }

    pub fn glyph(&self, ch: char) -> Option<u16> {
        self.cmap.get(&(ch as u32)).copied()
    }

    /// Advance in font units.
    pub fn advance(&self, gid: u16) -> u16 {
        let hmtx = self.table(b"hmtx");
        let index = (gid as usize).min(self.num_h_metrics.saturating_sub(1) as usize);
        u16_at(hmtx, index * 4)
    }

    pub fn kerning(&self, left: u16, right: u16) -> i16 {
        self.kern.get(&(left, right)).copied().unwrap_or(0)
    }

    /// The font cut down to the given glyphs (and whatever they are built
    /// from). Glyph numbers are kept, so text already encoded against the whole
    /// face still addresses the right outlines; the glyphs not wanted are
    /// emptied rather than renumbered.
    pub fn subset(&self, wanted: &BTreeSet<u16>) -> Vec<u8> {
        let loca = self.table(b"loca");
        let glyf = self.table(b"glyf");
        let long = i16_at(self.table(b"head"), 50) != 0;
        let range = |gid: u16| -> (usize, usize) {
            let g = gid as usize;
            if long {
                (
                    u32_at(loca, g * 4) as usize,
                    u32_at(loca, g * 4 + 4) as usize,
                )
            } else {
                (
                    u16_at(loca, g * 2) as usize * 2,
                    u16_at(loca, g * 2 + 2) as usize * 2,
                )
            }
        };

        // Composite glyphs name other glyphs; the closure includes them.
        let mut keep: BTreeSet<u16> = wanted.clone();
        keep.insert(0);
        let mut queue: Vec<u16> = keep.iter().copied().collect();
        while let Some(gid) = queue.pop() {
            if gid >= self.num_glyphs {
                continue;
            }
            let (s, e) = range(gid);
            let Some(g) = glyf.get(s..e) else { continue };
            if g.len() < 10 || i16_at(g, 0) >= 0 {
                continue;
            }
            let mut at = 10;
            loop {
                let flags = u16_at(g, at);
                let part = u16_at(g, at + 2);
                if keep.insert(part) {
                    queue.push(part);
                }
                at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
                at += if flags & 0x0008 != 0 {
                    2
                } else if flags & 0x0040 != 0 {
                    4
                } else if flags & 0x0080 != 0 {
                    8
                } else {
                    0
                };
                if flags & 0x0020 == 0 {
                    break;
                }
            }
        }

        let mut new_glyf: Vec<u8> = Vec::new();
        let mut new_loca: Vec<u8> = Vec::with_capacity((self.num_glyphs as usize + 1) * 4);
        for gid in 0..self.num_glyphs {
            new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
            if keep.contains(&gid) {
                let (s, e) = range(gid);
                if let Some(g) = glyf.get(s..e) {
                    new_glyf.extend_from_slice(g);
                    while !new_glyf.len().is_multiple_of(4) {
                        new_glyf.push(0);
                    }
                }
            }
        }
        new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());

        let mut head = self.table(b"head").to_vec();
        head[8..12].copy_from_slice(&[0; 4]); // checkSumAdjustment, set below
        head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

        let mut parts: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"cvt ", self.table(b"cvt ").to_vec()),
            (*b"fpgm", self.table(b"fpgm").to_vec()),
            (*b"glyf", new_glyf),
            (*b"head", head),
            (*b"hhea", self.table(b"hhea").to_vec()),
            (*b"hmtx", self.table(b"hmtx").to_vec()),
            (*b"loca", new_loca),
            (*b"maxp", self.table(b"maxp").to_vec()),
            (*b"prep", self.table(b"prep").to_vec()),
        ];
        parts.retain(|(_, body)| !body.is_empty());

        let n = parts.len() as u16;
        let mut search = 1u16;
        let mut shift = 0u16;
        while search * 2 <= n {
            search *= 2;
            shift += 1;
        }
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&n.to_be_bytes());
        out.extend_from_slice(&(search * 16).to_be_bytes());
        out.extend_from_slice(&shift.to_be_bytes());
        out.extend_from_slice(&(n * 16 - search * 16).to_be_bytes());

        let mut offset = 12 + parts.len() * 16;
        let mut directory = Vec::new();
        let mut body = Vec::new();
        let mut head_at = 0;
        for (tag, data) in &parts {
            if tag == b"head" {
                head_at = offset;
            }
            directory.extend_from_slice(tag);
            directory.extend_from_slice(&checksum(data).to_be_bytes());
            directory.extend_from_slice(&(offset as u32).to_be_bytes());
            directory.extend_from_slice(&(data.len() as u32).to_be_bytes());
            body.extend_from_slice(data);
            while body.len() % 4 != 0 {
                body.push(0);
            }
            offset = 12 + parts.len() * 16 + body.len();
        }
        out.extend_from_slice(&directory);
        out.extend_from_slice(&body);
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[head_at + 8..head_at + 12].copy_from_slice(&adjustment.to_be_bytes());
        out
    }

    /// The PostScript name, as the PDF wants it.
    pub fn postscript_name(&self) -> String {
        let face = match self.face {
            Face::Regular => "Regular",
            Face::Italic => "Italic",
            Face::Bold => "Bold",
            Face::BoldItalic => "BoldItalic",
        };
        format!("{}-{face}", self.family.base_name())
    }
}

fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// The parsed face, shared by every export in the process.
pub fn font(family: Family, face: Face) -> &'static Font {
    static FONTS: OnceLock<HashMap<(Family, Face), Font>> = OnceLock::new();
    let all = FONTS.get_or_init(|| {
        let mut map = HashMap::new();
        for family in [Family::Serif, Family::Sans] {
            for face in Face::ALL {
                map.insert(
                    (family, face),
                    Font::parse(bytes_of(family, face), family, face),
                );
            }
        }
        map
    });
    &all[&(family, face)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_faces_parse_and_know_their_glyphs() {
        for family in [Family::Serif, Family::Sans] {
            for face in Face::ALL {
                let f = font(family, face);
                assert_eq!(f.units_per_em, 2048);
                assert!(f.num_glyphs > 500);
                assert!(f.glyph('A').is_some() && f.glyph('é').is_some() && f.glyph('“').is_some());
                assert!(f.glyph('—').is_some() && f.glyph('…').is_some());
                // A space has width, and a capital is wider than an i.
                let space = f.advance(f.glyph(' ').unwrap());
                assert!(space > 300 && space < 1200, "{space}");
                assert!(f.advance(f.glyph('M').unwrap()) > f.advance(f.glyph('i').unwrap()));
                assert!(f.ascent > 0 && f.descent < 0);
            }
        }
    }

    #[test]
    fn widths_match_times() {
        // Liberation Serif is metric-compatible with Times New Roman, whose
        // lowercase "a" is 0.444 em and "M" is 0.889 em.
        let f = font(Family::Serif, Face::Regular);
        let em = f.units_per_em as f32;
        let a = f.advance(f.glyph('a').unwrap()) as f32 / em;
        let m = f.advance(f.glyph('M').unwrap()) as f32 / em;
        assert!((a - 0.444).abs() < 0.005, "{a}");
        assert!((m - 0.889).abs() < 0.005, "{m}");
    }

    #[test]
    fn pairs_that_kern_do() {
        let f = font(Family::Serif, Face::Regular);
        let (a, v) = (f.glyph('A').unwrap(), f.glyph('V').unwrap());
        assert!(f.kerning(a, v) < 0, "AV pulls together");
        let (n, o) = (f.glyph('n').unwrap(), f.glyph('o').unwrap());
        assert_eq!(f.kerning(n, o), 0);
    }

    #[test]
    fn a_subset_is_much_smaller_and_still_a_font() {
        let f = font(Family::Serif, Face::Regular);
        let wanted: BTreeSet<u16> = "Hello, world".chars().filter_map(|c| f.glyph(c)).collect();
        let subset = f.subset(&wanted);
        assert!(
            subset.len() < bytes_of(Family::Serif, Face::Regular).len() / 4,
            "{}",
            subset.len()
        );
        assert_eq!(&subset[..4], &[0, 1, 0, 0], "a TrueType file");
        // The whole-file checksum rule: with the adjustment in place the sum is fixed.
        assert_eq!(checksum(&subset), 0xB1B0_AFBA);
        // Glyph numbers survive: the table directory still says how many glyphs there are.
        let tables = u16_at(&subset, 4) as usize;
        let mut maxp = None;
        for i in 0..tables {
            let at = 12 + i * 16;
            if &subset[at..at + 4] == b"maxp" {
                maxp = Some(u32_at(&subset, at + 8) as usize);
            }
        }
        assert_eq!(u16_at(&subset, maxp.unwrap() + 4), f.num_glyphs);
    }

    #[test]
    fn accented_letters_that_are_composites_bring_their_parts() {
        let f = font(Family::Serif, Face::Regular);
        let e_acute = f.glyph('é').unwrap();
        let plain = f.subset(&BTreeSet::from([f.glyph('e').unwrap()]));
        let accent = f.subset(&BTreeSet::from([e_acute]));
        // The accented glyph needs its base and its mark, so it is the larger.
        assert!(accent.len() > plain.len());
    }
}
