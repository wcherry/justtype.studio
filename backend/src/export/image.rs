//! Getting a cover image into a PDF.
//!
//! A PDF can carry a JPEG as it is and a PNG almost as it is: the PNG's own
//! compressed data goes in with a predictor that says how its rows were
//! filtered. Only an image with transparency, or one with more than eight
//! bits a sample, has to be taken apart and put back together, because PDF
//! keeps the alpha channel as a second image.
use std::io::{Read, Write};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use super::Cover;

/// An image as the PDF writer needs it.
#[derive(Debug)]
pub struct PdfImage {
    pub width: u32,
    pub height: u32,
    /// The PDF colour space operand, as written.
    pub color_space: String,
    pub bits: u8,
    /// `DCTDecode` or `FlateDecode`.
    pub filter: &'static str,
    /// For PNG data kept as it was: the predictor that undoes its row filters.
    pub decode_parms: Option<String>,
    pub data: Vec<u8>,
    /// The alpha channel, as a greyscale image of its own.
    pub alpha: Option<Box<PdfImage>>,
}

pub fn load(cover: &Cover) -> Result<PdfImage, String> {
    match cover.mime.as_str() {
        "image/jpeg" => jpeg(&cover.bytes),
        "image/png" => png(&cover.bytes),
        other => Err(format!("{other} cannot go in a PDF")),
    }
}

fn jpeg(bytes: &[u8]) -> Result<PdfImage, String> {
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF || (0xD0..=0xD8).contains(&marker) || marker == 0x01 {
            i += if marker == 0xFF { 1 } else { 2 };
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            let color_space = match bytes[i + 9] {
                1 => "/DeviceGray",
                3 => "/DeviceRGB",
                n => {
                    return Err(format!(
                        "a JPEG with {n} colour channels (CMYK) is not supported"
                    ))
                }
            };
            return Ok(PdfImage {
                width,
                height,
                color_space: color_space.into(),
                bits: bytes[i + 4],
                filter: "DCTDecode",
                decode_parms: None,
                data: bytes.to_vec(),
                alpha: None,
            });
        }
        i += 2 + len;
    }
    Err("that JPEG has no frame header".into())
}

/// A PNG chunk: its type and its data.
type Chunk<'a> = (&'a [u8], &'a [u8]);

fn chunks(bytes: &[u8]) -> Result<Vec<Chunk<'_>>, String> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("not a PNG".into());
    }
    let mut out = Vec::new();
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize;
        let kind = &bytes[at + 4..at + 8];
        let body = bytes
            .get(at + 8..at + 8 + len)
            .ok_or("the PNG is cut short")?;
        out.push((kind, body));
        at += 12 + len;
    }
    Ok(out)
}

fn png(bytes: &[u8]) -> Result<PdfImage, String> {
    let parts = chunks(bytes)?;
    let header = parts
        .iter()
        .find(|(k, _)| *k == b"IHDR")
        .ok_or("the PNG has no header")?
        .1;
    let width = u32::from_be_bytes(header[0..4].try_into().expect("four bytes"));
    let height = u32::from_be_bytes(header[4..8].try_into().expect("four bytes"));
    let (depth, color, interlace) = (header[8], header[9], header[12]);
    if interlace != 0 {
        return Err("an interlaced PNG is not supported; save it without interlacing".into());
    }
    let idat: Vec<u8> = parts
        .iter()
        .filter(|(k, _)| *k == b"IDAT")
        .flat_map(|(_, b)| b.iter().copied())
        .collect();

    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err("an unknown PNG colour type".into()),
    };

    // Kept as it is: no alpha, and a depth PDF takes as it stands.
    if matches!(color, 0 | 2 | 3) && depth <= 8 {
        let color_space = match color {
            0 => "/DeviceGray".to_string(),
            2 => "/DeviceRGB".to_string(),
            _ => {
                let palette = parts
                    .iter()
                    .find(|(k, _)| *k == b"PLTE")
                    .ok_or("the PNG has no palette")?
                    .1;
                format!(
                    "[/Indexed /DeviceRGB {} <{}>]",
                    palette.len() / 3 - 1,
                    hex::encode(palette)
                )
            }
        };
        return Ok(PdfImage {
            width,
            height,
            color_space,
            bits: depth,
            filter: "FlateDecode",
            decode_parms: Some(format!(
                "<< /Predictor 15 /Colors {channels} /BitsPerComponent {depth} /Columns {width} >>"
            )),
            data: idat,
            alpha: None,
        });
    }

    // Otherwise take it apart: inflate, undo the row filters, split the planes.
    let mut raw = Vec::new();
    ZlibDecoder::new(&idat[..])
        .read_to_end(&mut raw)
        .map_err(|e| format!("the PNG's data is damaged: {e}"))?;
    let sample = if depth == 16 { 2 } else { 1 };
    if depth != 8 && depth != 16 {
        return Err("a PNG with that bit depth is not supported".into());
    }
    let bpp = channels * sample;
    let stride = width as usize * bpp;
    if raw.len() < (stride + 1) * height as usize {
        return Err("the PNG's data is shorter than its size says".into());
    }
    let rows = unfilter(&raw, stride, bpp, height as usize)?;

    let colour_channels = if color == 4 || color == 0 { 1 } else { 3 };
    let has_alpha = color == 4 || color == 6;
    let mut colour = Vec::with_capacity(width as usize * height as usize * colour_channels);
    let mut alpha = Vec::new();
    for row in rows.chunks(stride) {
        for px in row.chunks(bpp) {
            // Sixteen bits a sample: the high byte is the sample at eight.
            for c in 0..colour_channels {
                colour.push(px[c * sample]);
            }
            if has_alpha {
                alpha.push(px[colour_channels * sample]);
            }
        }
    }
    let deflate = |data: &[u8]| -> Result<Vec<u8>, String> {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).map_err(|e| e.to_string())?;
        enc.finish().map_err(|e| e.to_string())
    };
    let alpha_image = if has_alpha {
        Some(Box::new(PdfImage {
            width,
            height,
            color_space: "/DeviceGray".into(),
            bits: 8,
            filter: "FlateDecode",
            decode_parms: None,
            data: deflate(&alpha)?,
            alpha: None,
        }))
    } else {
        None
    };
    Ok(PdfImage {
        width,
        height,
        color_space: if colour_channels == 1 {
            "/DeviceGray"
        } else {
            "/DeviceRGB"
        }
        .into(),
        bits: 8,
        filter: "FlateDecode",
        decode_parms: None,
        data: deflate(&colour)?,
        alpha: alpha_image,
    })
}

/// Reverses PNG's five row filters.
fn unfilter(raw: &[u8], stride: usize, bpp: usize, height: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; stride * height];
    for y in 0..height {
        let filter = raw[y * (stride + 1)];
        let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
        for x in 0..stride {
            let left = if x >= bpp {
                out[y * stride + x - bpp]
            } else {
                0
            };
            let up = if y > 0 { out[(y - 1) * stride + x] } else { 0 };
            let up_left = if y > 0 && x >= bpp {
                out[(y - 1) * stride + x - bpp]
            } else {
                0
            };
            let predicted = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((left as u16 + up as u16) / 2) as u8,
                4 => {
                    let (a, b, c) = (left as i32, up as i32, up_left as i32);
                    let p = a + b - c;
                    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                    if pa <= pb && pa <= pc {
                        left
                    } else if pb <= pc {
                        up
                    } else {
                        up_left
                    }
                }
                _ => return Err("the PNG uses a row filter that does not exist".into()),
            };
            out[y * stride + x] = line[x].wrapping_add(predicted);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc(kind: &[u8], body: &[u8]) -> u32 {
        // CRC-32 of the type and data, as PNG wants — computed without a crate.
        let mut table = [0u32; 256];
        for n in 0..256u32 {
            let mut c = n;
            for _ in 0..8 {
                c = if c & 1 == 1 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            table[n as usize] = c;
        }
        let mut c = 0xFFFF_FFFFu32;
        for b in kind.iter().chain(body) {
            c = table[((c ^ *b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }

    fn png_of(width: u32, height: u32, depth: u8, color: u8, rows: &[Vec<u8>]) -> Vec<u8> {
        let mut raw = Vec::new();
        for r in rows {
            raw.push(0); // no filter
            raw.extend_from_slice(r);
        }
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&raw).unwrap();
        let idat = z.finish().unwrap();
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[depth, color, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, body) in [(&b"IHDR"[..], ihdr), (b"IDAT", idat), (b"IEND", vec![])] {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(kind, &body).to_be_bytes());
        }
        out
    }

    fn cover(mime: &str, bytes: Vec<u8>) -> Cover {
        Cover {
            bytes,
            mime: mime.into(),
        }
    }

    #[test]
    fn an_rgb_png_goes_in_as_it_is_with_a_predictor() {
        let png = png_of(
            2,
            2,
            8,
            2,
            &[vec![255, 0, 0, 0, 255, 0], vec![0, 0, 255, 9, 9, 9]],
        );
        let image = load(&cover("image/png", png)).unwrap();
        assert_eq!((image.width, image.height, image.bits), (2, 2, 8));
        assert_eq!(image.color_space, "/DeviceRGB");
        assert!(image
            .decode_parms
            .as_ref()
            .unwrap()
            .contains("/Predictor 15"));
        assert!(image.alpha.is_none());
    }

    #[test]
    fn transparency_is_split_into_its_own_image() {
        // RGBA: one red pixel half transparent, one blue and opaque.
        let png = png_of(2, 1, 8, 6, &[vec![255, 0, 0, 128, 0, 0, 255, 255]]);
        let image = load(&cover("image/png", png)).unwrap();
        let alpha = image.alpha.as_ref().expect("an alpha plane");
        let inflate = |d: &[u8]| {
            let mut out = Vec::new();
            ZlibDecoder::new(d).read_to_end(&mut out).unwrap();
            out
        };
        assert_eq!(inflate(&image.data), [255, 0, 0, 0, 0, 255]);
        assert_eq!(inflate(&alpha.data), [128, 255]);
        assert!(image.decode_parms.is_none());
    }

    #[test]
    fn sixteen_bit_samples_are_reduced_to_eight() {
        let png = png_of(1, 1, 16, 2, &[vec![0xAB, 0xCD, 0x12, 0x34, 0xFF, 0x00]]);
        let image = load(&cover("image/png", png)).unwrap();
        let mut out = Vec::new();
        ZlibDecoder::new(&image.data[..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, [0xAB, 0x12, 0xFF]);
    }

    #[test]
    fn a_jpeg_is_carried_whole_with_its_size() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        jpeg.extend_from_slice(&200u16.to_be_bytes());
        jpeg.extend_from_slice(&100u16.to_be_bytes());
        jpeg.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        let image = load(&cover("image/jpeg", jpeg.clone())).unwrap();
        assert_eq!((image.width, image.height), (100, 200));
        assert_eq!(image.filter, "DCTDecode");
        assert_eq!(image.data, jpeg);
        assert_eq!(image.color_space, "/DeviceRGB");
    }

    #[test]
    fn what_cannot_be_carried_says_so() {
        assert!(load(&cover("image/gif", vec![])).is_err());
        assert!(load(&cover("image/png", b"nope".to_vec())).is_err());
        let mut interlaced = png_of(1, 1, 8, 2, &[vec![0, 0, 0]]);
        interlaced[28] = 1; // the interlace byte of IHDR
        assert!(load(&cover("image/png", interlaced))
            .unwrap_err()
            .contains("interlaced"));
    }

    #[test]
    fn row_filters_are_undone() {
        // Two rows of three grey samples: Sub on the first, Up on the second.
        let raw = [1, 10, 5, 5, 2, 1, 1, 1];
        assert_eq!(unfilter(&raw, 3, 1, 2).unwrap(), [10, 15, 20, 11, 16, 21]);
    }
}
