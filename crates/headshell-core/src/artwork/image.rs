//! Checking and resizing a cover image (D-076).
//!
//! Every image the chain finds passes through here before it is kept: a file
//! a tag calls a picture, or bytes a server sent, are not trusted to be one.
//! The format and the size are read from the header **without decoding**, so
//! an oversized image is refused before it costs memory.
//!
//! With the `artwork-resize` feature, JPEG and PNG are decoded and two PNG
//! variants are made: `label` (320 px, the record's label) and `thumb` (96 px,
//! a queue row). The downscale is an area average — each target pixel is the
//! mean of the source pixels it covers — so a 1500 px cover shrinking to
//! 96 px does not alias the way a four-tap filter would. Without the feature
//! (mobile), or for a format we do not decode, the image is passed on as it
//! is — up to [`PASS_THROUGH_LIMIT`].

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};

/// The largest image read at all. A cover is rarely over 2 MB; 16 MB lets a
/// lossless scan through and stops a file that only claims to be a picture.
pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;

/// The largest edge decoded. Past it the decoded pixels alone would take
/// hundreds of megabytes.
pub(crate) const MAX_EDGE: u32 = 8000;

/// The record's label: ~150 px on screen, twice that for a 2× display.
#[cfg(feature = "artwork-resize")]
pub(crate) const LABEL_EDGE: u32 = 320;

/// A queue row's square: 36 px on screen; 96 covers a 2× display.
#[cfg(feature = "artwork-resize")]
pub(crate) const THUMB_EDGE: u32 = 96;

/// The most that goes to the interface without being resized. Both variants
/// are the same bytes then, and each one crosses the IPC bridge.
pub(crate) const PASS_THROUGH_LIMIT: usize = 2 * 1024 * 1024;

/// The formats a cover arrives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Jpeg,
    Png,
    Gif,
    Webp,
}

impl Format {
    pub(crate) const fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }

    /// The reverse of [`Self::extension`], for reading the cache back.
    pub(crate) fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "jpg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// What the header says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Probe {
    pub(crate) format: Format,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// One kept image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Variant {
    pub(crate) bytes: Vec<u8>,
    pub(crate) format: Format,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The two variants a cover is kept as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Normalized {
    pub(crate) label: Variant,
    pub(crate) thumb: Variant,
}

fn unusable(detail: impl Into<String>) -> Error {
    Error::new(
        Stage::ArtworkRead,
        ErrorKind::Artwork {
            detail: detail.into(),
        },
    )
}

/// Reads the format and the size from the header, without decoding.
///
/// # Errors
/// If the bytes are not an image in a format we know, or the header is cut
/// short.
pub(crate) fn probe(bytes: &[u8]) -> Result<Probe> {
    let found = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        jpeg_size(bytes).map(|(width, height)| (Format::Jpeg, width, height))
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_size(bytes).map(|(width, height)| (Format::Png, width, height))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        gif_size(bytes).map(|(width, height)| (Format::Gif, width, height))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        webp_size(bytes).map(|(width, height)| (Format::Webp, width, height))
    } else {
        return Err(unusable(format!(
            "not an image in a format we read (JPEG, PNG, GIF, WebP); it starts with {}",
            hex_prefix(bytes)
        )));
    };
    match found {
        Some((format, width, height)) if width > 0 && height > 0 => Ok(Probe {
            format,
            width,
            height,
        }),
        Some((format, ..)) => Err(unusable(format!(
            "a {} header with a zero size",
            format.mime()
        ))),
        None => Err(unusable("an image header that is cut short or broken")),
    }
}

/// Checks the limits and makes the two variants.
///
/// # Errors
/// If the bytes are not an image, are over a limit, or do not decode.
pub(crate) fn normalize(bytes: &[u8]) -> Result<Normalized> {
    if bytes.len() > MAX_BYTES {
        return Err(unusable(format!(
            "{} — over the {} MB limit",
            megabytes(bytes.len()),
            MAX_BYTES / (1024 * 1024)
        )));
    }
    let probe = probe(bytes)?;
    if probe.width > MAX_EDGE || probe.height > MAX_EDGE {
        return Err(unusable(format!(
            "{}×{} px — over the {MAX_EDGE} px limit",
            probe.width, probe.height
        )));
    }

    #[cfg(feature = "artwork-resize")]
    if matches!(probe.format, Format::Jpeg | Format::Png) {
        return resize::variants(bytes, probe);
    }

    pass_through(bytes, probe)
}

/// The image as it is, as both variants: when it cannot be resized here.
fn pass_through(bytes: &[u8], probe: Probe) -> Result<Normalized> {
    if bytes.len() > PASS_THROUGH_LIMIT {
        return Err(unusable(format!(
            "{} {} that this build does not resize, over the {} MB it passes on as it is",
            megabytes(bytes.len()),
            probe.format.mime(),
            PASS_THROUGH_LIMIT / (1024 * 1024)
        )));
    }
    let variant = Variant {
        bytes: bytes.to_vec(),
        format: probe.format,
        width: probe.width,
        height: probe.height,
    };
    Ok(Normalized {
        label: variant.clone(),
        thumb: variant,
    })
}

fn megabytes(bytes: usize) -> String {
    // One decimal is enough for a message; the exact count is not the point.
    let tenths = bytes.saturating_mul(10) / (1024 * 1024);
    format!("{}.{} MB", tenths / 10, tenths % 10)
}

fn hex_prefix(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "nothing (0 bytes)".to_owned();
    }
    bytes
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    let pair = bytes.get(at..at + 2)?;
    Some(u32::from(u16::from_be_bytes([pair[0], pair[1]])))
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    let pair = bytes.get(at..at + 2)?;
    Some(u32::from(u16::from_le_bytes([pair[0], pair[1]])))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let triple = bytes.get(at..at + 3)?;
    Some(u32::from(triple[0]) | (u32::from(triple[1]) << 8) | (u32::from(triple[2]) << 16))
}

/// The size in the first SOF segment. The segments before it are skipped by
/// their length; a scan (`SOS`) or the end (`EOI`) before any SOF is a broken
/// file.
fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        // Markers may be padded with any number of 0xFF.
        while *bytes.get(at)? == 0xFF && *bytes.get(at + 1)? == 0xFF {
            at += 1;
        }
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        at += 2;
        match marker {
            // Standalone markers carry no length.
            0x01 | 0xD0..=0xD8 => continue,
            0xD9 | 0xDA => return None,
            // SOF0–SOF15, except DHT (C4), JPG (C8) and DAC (CC).
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let height = be16(bytes, at + 3)?;
                let width = be16(bytes, at + 5)?;
                return Some((width, height));
            }
            _ => {
                let length = usize::try_from(be16(bytes, at)?).ok()?;
                if length < 2 {
                    return None;
                }
                at += length;
            }
        }
    }
}

/// The size in `IHDR`, which must be the first chunk.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((width, height))
}

/// The logical screen size.
fn gif_size(bytes: &[u8]) -> Option<(u32, u32)> {
    Some((le16(bytes, 6)?, le16(bytes, 8)?))
}

/// The size in whichever of the three WebP bitstream headers comes first.
fn webp_size(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        // Extended: the canvas size, stored minus one, 24 bits each.
        b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
        // Lossy: after the 3-byte frame tag and the 9d 01 2a start code,
        // 14 bits each (the top two bits are a scale).
        b"VP8 " => {
            if bytes.get(23..26)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            Some((le16(bytes, 26)? & 0x3fff, le16(bytes, 28)? & 0x3fff))
        }
        // Lossless: after the 0x2f signature, 14 bits each, stored minus one.
        b"VP8L" => {
            if *bytes.get(20)? != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}

#[cfg(feature = "artwork-resize")]
mod resize {
    use super::{Format, LABEL_EDGE, MAX_EDGE, Normalized, Probe, THUMB_EDGE, Variant, unusable};
    use crate::error::Result;

    /// Decoded pixels, 8 bits a channel, 3 (RGB) or 4 (RGBA) channels.
    struct Pixels {
        data: Vec<u8>,
        width: u32,
        height: u32,
        channels: usize,
    }

    pub(super) fn variants(bytes: &[u8], probe: Probe) -> Result<Normalized> {
        let pixels = match probe.format {
            Format::Jpeg => decode_jpeg(bytes)?,
            Format::Png => decode_png(bytes)?,
            // `normalize` only sends these two here.
            Format::Gif | Format::Webp => {
                return Err(unusable("this format is not decoded here"));
            }
        };
        let label = variant(bytes, probe, &pixels, LABEL_EDGE)?;
        let thumb = variant(bytes, probe, &pixels, THUMB_EDGE)?;
        Ok(Normalized { label, thumb })
    }

    /// An image already within the edge is kept as it came — re-encoding a
    /// small JPEG as PNG would only make it bigger.
    fn variant(original: &[u8], probe: Probe, pixels: &Pixels, edge: u32) -> Result<Variant> {
        if pixels.width.max(pixels.height) <= edge {
            return Ok(Variant {
                bytes: original.to_vec(),
                format: probe.format,
                width: pixels.width,
                height: pixels.height,
            });
        }
        let (width, height) = fit(pixels.width, pixels.height, edge);
        let data = downscale(pixels, width, height);
        let bytes = encode_png(&data, width, height, pixels.channels)?;
        Ok(Variant {
            bytes,
            format: Format::Png,
            width,
            height,
        })
    }

    /// The largest size within `edge` × `edge` with the same aspect.
    fn fit(width: u32, height: u32, edge: u32) -> (u32, u32) {
        let scale = f64::from(edge) / f64::from(width.max(height));
        let side = |length: u32| {
            // Rounded, and at least one pixel: a 3000×2 strip still has a row.
            let scaled = (f64::from(length) * scale).round();
            if scaled < 1.0 { 1 } else { scaled as u32 }
        };
        (side(width), side(height))
    }

    fn decode_jpeg(bytes: &[u8]) -> Result<Pixels> {
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;

        let edge = MAX_EDGE as usize;
        let options = DecoderOptions::default()
            .jpeg_set_out_colorspace(ColorSpace::RGB)
            .set_max_width(edge)
            .set_max_height(edge);
        let mut decoder = JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
        let data = decoder
            .decode()
            .map_err(|err| unusable(format!("the JPEG did not decode: {err:?}")))?;
        let info = decoder
            .info()
            .ok_or_else(|| unusable("the JPEG decoded but reported no size"))?;
        let (width, height) = (u32::from(info.width), u32::from(info.height));
        let channels = match decoder.output_colorspace() {
            Some(space) => space.num_components(),
            None => 3,
        };
        let data = match channels {
            3 => data,
            // A greyscale JPEG the decoder kept as one channel.
            1 => data.iter().flat_map(|&v| [v, v, v]).collect(),
            other => {
                return Err(unusable(format!(
                    "the JPEG decoded into {other} channels, not RGB"
                )));
            }
        };
        Ok(Pixels {
            data,
            width,
            height,
            channels: 3,
        })
    }

    fn decode_png(bytes: &[u8]) -> Result<Pixels> {
        use png::{ColorType, Decoder, Limits, Transformations};

        let edge = MAX_EDGE as usize;
        let mut decoder = Decoder::new_with_limits(
            std::io::Cursor::new(bytes),
            Limits {
                bytes: edge * edge * 4,
            },
        );
        decoder.set_transformations(Transformations::normalize_to_color8());
        let broken = |err: png::DecodingError| unusable(format!("the PNG did not decode: {err}"));
        let mut reader = decoder.read_info().map_err(broken)?;
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| unusable("the PNG is too large to decode"))?;
        let mut data = vec![0; size];
        let frame = reader.next_frame(&mut data).map_err(broken)?;
        data.truncate(frame.buffer_size());
        let (width, height) = (frame.width, frame.height);
        let (data, channels) = match reader.output_color_type().0 {
            ColorType::Rgb => (data, 3),
            ColorType::Rgba => (data, 4),
            ColorType::Grayscale => (data.iter().flat_map(|&v| [v, v, v]).collect(), 3),
            ColorType::GrayscaleAlpha => (
                data.chunks_exact(2)
                    .flat_map(|pair| [pair[0], pair[0], pair[0], pair[1]])
                    .collect(),
                4,
            ),
            // `EXPAND` turns a palette into RGB(A); it does not reach here.
            ColorType::Indexed => return Err(unusable("a palette PNG that was not expanded")),
        };
        Ok(Pixels {
            data,
            width,
            height,
            channels,
        })
    }

    /// Area averaging, one axis at a time: every target pixel is the mean of
    /// the source pixels under it, the edge pixels weighted by how much of
    /// them it covers.
    fn downscale(pixels: &Pixels, width: u32, height: u32) -> Vec<u8> {
        let channels = pixels.channels;
        let horizontal = shrink_axis(
            &pixels.data,
            pixels.width as usize,
            pixels.height as usize,
            channels,
            width as usize,
            true,
        );
        let vertical = shrink_axis(
            &horizontal,
            width as usize,
            pixels.height as usize,
            channels,
            height as usize,
            false,
        );
        vertical
            .iter()
            .map(|&value| value.round().clamp(0.0, 255.0) as u8)
            .collect()
    }

    /// Shrinks rows (`along_x`) or columns to `target` samples.
    fn shrink_axis(
        data: &[impl Copy + Into<f32>],
        width: usize,
        height: usize,
        channels: usize,
        target: usize,
        along_x: bool,
    ) -> Vec<f32> {
        let (source, lines) = if along_x {
            (width, height)
        } else {
            (height, width)
        };
        let (out_width, out_height) = if along_x {
            (target, height)
        } else {
            (width, target)
        };
        let mut out = vec![0.0f32; out_width * out_height * channels];
        let ratio = source as f32 / target as f32;
        let index = |line: usize, position: usize| {
            if along_x {
                (line * width + position) * channels
            } else {
                (position * width + line) * channels
            }
        };
        let out_index = |line: usize, position: usize| {
            if along_x {
                (line * out_width + position) * channels
            } else {
                (position * out_width + line) * channels
            }
        };
        for line in 0..lines {
            for position in 0..target {
                let start = position as f32 * ratio;
                let end = start + ratio;
                let first = start.floor() as usize;
                let last = (end.ceil() as usize).min(source);
                let mut sum = [0.0f32; 4];
                let mut weight = 0.0f32;
                for sample in first..last {
                    let covered =
                        (end.min(sample as f32 + 1.0) - start.max(sample as f32)).max(0.0);
                    if covered <= 0.0 {
                        continue;
                    }
                    let at = index(line, sample);
                    for (total, value) in sum.iter_mut().zip(&data[at..at + channels]) {
                        *total += (*value).into() * covered;
                    }
                    weight += covered;
                }
                let at = out_index(line, position);
                for (slot, total) in out[at..at + channels].iter_mut().zip(sum) {
                    *slot = if weight > 0.0 { total / weight } else { 0.0 };
                }
            }
        }
        out
    }

    fn encode_png(data: &[u8], width: u32, height: u32, channels: usize) -> Result<Vec<u8>> {
        use png::{BitDepth, ColorType, Encoder};

        // An alpha channel that is opaque everywhere only costs bytes.
        let opaque = channels == 4 && data.chunks_exact(4).all(|px| px[3] == 255);
        let (pixels, color) = if channels == 4 && !opaque {
            (data.to_vec(), ColorType::Rgba)
        } else if channels == 4 {
            let rgb = data
                .chunks_exact(4)
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect();
            (rgb, ColorType::Rgb)
        } else {
            (data.to_vec(), ColorType::Rgb)
        };

        let failed = |err: png::EncodingError| unusable(format!("could not write the PNG: {err}"));
        let mut out = Vec::new();
        let mut encoder = Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(failed)?;
        writer.write_image_data(&pixels).map_err(failed)?;
        writer.finish().map_err(failed)?;
        Ok(out)
    }

    /// Decodes a PNG to RGB — for the tests outside this module.
    #[cfg(test)]
    pub(crate) fn decode_for_test(bytes: &[u8]) -> Vec<u8> {
        decode_png(bytes).unwrap().data
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fitting_keeps_the_aspect_and_never_reaches_zero() {
            assert_eq!(fit(1500, 1500, 320), (320, 320));
            assert_eq!(fit(1600, 800, 320), (320, 160));
            assert_eq!(fit(800, 1600, 96), (48, 96));
            assert_eq!(fit(3000, 2, 96), (96, 1));
        }

        #[test]
        fn the_area_average_is_the_mean_of_what_it_covers() {
            // A 4×1 row: two black, two white. Halved, each target pixel covers
            // exactly one pair; quartered to one pixel, it is the grey mean.
            let pixels = Pixels {
                data: vec![0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255],
                width: 4,
                height: 1,
                channels: 3,
            };
            assert_eq!(downscale(&pixels, 2, 1), vec![0, 0, 0, 255, 255, 255]);
            assert_eq!(downscale(&pixels, 1, 1), vec![128, 128, 128]);
        }

        #[test]
        fn a_fractional_ratio_weighs_the_edge_pixels() {
            // 3 → 2: the middle pixel is shared half and half.
            let pixels = Pixels {
                data: vec![0, 0, 0, 90, 90, 90, 180, 180, 180],
                width: 3,
                height: 1,
                channels: 3,
            };
            // First target: 1.0 × 0 + 0.5 × 90 over 1.5 = 30; second: 0.5 × 90 + 1.0
            // × 180 over 1.5 = 150.
            assert_eq!(downscale(&pixels, 2, 1), vec![30, 30, 30, 150, 150, 150]);
        }
    }
}

/// Real images for tests, written by hand so no encoder is needed to make
/// one: a PNG with stored (uncompressed) deflate blocks.
#[cfg(test)]
pub(crate) mod tests_support {
    /// A `width`×`height` PNG of one colour.
    pub(crate) fn png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        // Every row: the filter byte (none), then the pixels.
        let row: Vec<u8> = std::iter::once(0)
            .chain(
                rgb.iter()
                    .copied()
                    .cycle()
                    .take(usize::try_from(width).unwrap() * 3),
            )
            .collect();
        let raw: Vec<u8> = (0..height).flat_map(|_| row.iter().copied()).collect();
        let mut zlib = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
        for (i, block) in blocks.iter().enumerate() {
            zlib.push(u8::from(i + 1 == blocks.len()));
            let length = u16::try_from(block.len()).unwrap();
            zlib.extend(length.to_le_bytes());
            zlib.extend((!length).to_le_bytes());
            zlib.extend(*block);
        }
        zlib.extend(adler32(&raw).to_be_bytes());

        let mut header = width.to_be_bytes().to_vec();
        header.extend(height.to_be_bytes());
        header.extend([8, 2, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut out, b"IHDR", &header);
        chunk(&mut out, b"IDAT", &zlib);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    /// The smallest real cover a folder can hold.
    pub(crate) fn png_2x2() -> Vec<u8> {
        png(2, 2, [200, 40, 40])
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend(crc32(&[kind.as_slice(), data].concat()).to_be_bytes());
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn adler32(bytes: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in bytes {
            a = (a + u32::from(byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        (b << 16) | a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG header: signature, IHDR length and name, then the size.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 2, 0, 0, 0]);
        bytes
    }

    #[test]
    fn the_size_is_read_from_each_formats_header() {
        assert_eq!(
            probe(&png_header(640, 480)).unwrap(),
            Probe {
                format: Format::Png,
                width: 640,
                height: 480
            }
        );

        let gif = [b"GIF89a".as_slice(), &[0x40, 0x01, 0xf0, 0x00]].concat();
        let gif = probe(&gif).unwrap();
        assert_eq!((gif.format, gif.width, gif.height), (Format::Gif, 320, 240));

        // A JPEG with an APP0 segment before the SOF0.
        let jpeg = [
            &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00][..],
            &[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80, 0x03],
        ]
        .concat();
        let jpeg = probe(&jpeg).unwrap();
        assert_eq!(
            (jpeg.format, jpeg.width, jpeg.height),
            (Format::Jpeg, 640, 480)
        );

        // WebP lossless: 0x2f, then width-1 and height-1 in 14 bits each.
        let bits: u32 = 199 | (99 << 14);
        let webp = [
            b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".as_slice(),
            &bits.to_le_bytes(),
        ]
        .concat();
        let webp = probe(&webp).unwrap();
        assert_eq!(
            (webp.format, webp.width, webp.height),
            (Format::Webp, 200, 100)
        );
    }

    #[test]
    fn what_is_not_an_image_is_said_so_with_its_first_bytes() {
        let err = probe(b"<html>").unwrap_err();
        let text = err.chain_text();
        assert!(text.starts_with("STEP: ARTWORK_READ"), "{text}");
        assert!(text.contains("3c 68 74 6d"), "{text}");
        assert!(probe(b"").unwrap_err().chain_text().contains("0 bytes"));
        // A header cut before its size is broken, not zero-sized.
        assert!(probe(&png_header(1, 1)[..18]).is_err());
    }

    #[test]
    fn an_image_over_a_limit_is_refused_before_it_is_decoded() {
        let err = normalize(&png_header(9000, 10)).unwrap_err();
        assert!(
            err.chain_text().contains("9000×10 px"),
            "{}",
            err.chain_text()
        );

        let huge = vec![0u8; MAX_BYTES + 1];
        let err = normalize(&huge).unwrap_err();
        assert!(err.chain_text().contains("16 MB"), "{}", err.chain_text());
    }

    #[test]
    fn a_format_not_decoded_here_passes_through_up_to_its_limit() {
        let gif = [b"GIF89a".as_slice(), &[0x10, 0x00, 0x10, 0x00]].concat();
        let kept = normalize(&gif).unwrap();
        assert_eq!(kept.label.bytes, gif);
        assert_eq!(kept.thumb.format, Format::Gif);

        let mut big = gif.clone();
        big.resize(PASS_THROUGH_LIMIT + 1, 0);
        assert!(normalize(&big).is_err());
    }

    #[test]
    fn a_hand_written_png_is_a_png() {
        let png = tests_support::png(3, 2, [1, 2, 3]);
        assert_eq!(
            probe(&png).unwrap(),
            Probe {
                format: Format::Png,
                width: 3,
                height: 2
            }
        );
    }

    #[cfg(feature = "artwork-resize")]
    #[test]
    fn a_large_cover_becomes_two_small_pngs_and_a_small_one_is_kept_as_it_is() {
        let large = tests_support::png(400, 200, [10, 120, 200]);
        let kept = normalize(&large).unwrap();
        assert_eq!((kept.label.width, kept.label.height), (320, 160));
        assert_eq!((kept.thumb.width, kept.thumb.height), (96, 48));
        // The resized variants are real PNGs, of the size they claim.
        let label = probe(&kept.label.bytes).unwrap();
        assert_eq!(
            (label.format, label.width, label.height),
            (Format::Png, 320, 160)
        );
        // One colour in, the same colour out: the average does not drift.
        let decoded = resize::decode_for_test(&kept.thumb.bytes);
        assert!(
            decoded.chunks_exact(3).all(|px| px == [10, 120, 200]),
            "{:?}",
            &decoded[..6]
        );

        let small = tests_support::png(80, 80, [0, 0, 0]);
        let kept = normalize(&small).unwrap();
        assert_eq!(
            kept.label.bytes, small,
            "within both edges: kept as it came"
        );
        assert_eq!(kept.thumb.bytes, small);
    }
}
