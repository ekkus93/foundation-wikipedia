//! PACK-003: shrink a verified image before it goes into a pack, without
//! losing a single pixel of what the reader shows.
//!
//! Owner decision 2026-11-17 ("lossless + still for big GIFs"):
//!
//! * a PNG or a single-frame GIF becomes lossless WebP when that is smaller.
//!   The WebP is decoded again and must give exactly the same RGBA pixels,
//!   so diagram labels are as legible as in the source. A PNG with an
//!   embedded colour profile, gamma or chromaticities other than sRGB's, an
//!   animated PNG, or 16-bit samples is kept as it is, because lossless WebP
//!   could not reproduce it exactly;
//! * an animated GIF larger than [`ANIMATION_STILL_BYTES`] is packed as its
//!   first frame (lossless WebP), and the reader labels it as a still whose
//!   animation is online only. A smaller animation is kept whole;
//! * JPEG and WebP are kept as they are; re-encoding them losslessly would
//!   only grow them.
//!
//! Anything that cannot be decoded within [`decode_limits`] is kept as the
//! verified original: re-encoding is an optimisation, never a reason to
//! change or drop an image.

use std::io::Cursor;

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPEncoder;
use image::{AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use wiki_model::MediaDerivation;

/// An animated GIF above this size is packed as a still (owner decision).
pub const ANIMATION_STILL_BYTES: usize = 2 * 1024 * 1024;

/// Largest decoded image the re-encoder will hold in memory.
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

/// The bytes to store in a pack for one verified image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    /// `None` when `bytes` are the downloaded file unchanged.
    pub derivation: Option<MediaDerivation>,
}

fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    limits
}

/// What to store for `bytes`, already verified as `mime`.
pub fn pack_image(bytes: Vec<u8>, mime: &'static str) -> PackedImage {
    let derived = match mime {
        "image/png" if png_is_plain(&bytes) => decode_png(&bytes)
            .and_then(|image| smaller_lossless(&image, bytes.len()))
            .map(|webp| (webp, MediaDerivation::LosslessWebp)),
        "image/gif" => gif(&bytes),
        _ => None,
    };
    match derived {
        Some((webp, derivation)) => PackedImage {
            bytes: webp,
            mime: "image/webp",
            derivation: Some(derivation),
        },
        None => PackedImage {
            bytes,
            mime,
            derivation: None,
        },
    }
}

/// A PNG whose pixels alone define what is shown, as in lossless WebP
/// (which is sRGB): no embedded colour profile, no animation, and gamma or
/// chromaticity only if they restate sRGB, as image editors often write.
fn png_is_plain(bytes: &[u8]) -> bool {
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        let kind = &bytes[at + 4..at + 8];
        let Some(next) = (length as usize)
            .checked_add(12)
            .and_then(|step| at.checked_add(step))
            .filter(|next| *next <= bytes.len())
        else {
            return false;
        };
        let data = &bytes[at + 8..next - 4];
        let plain = match kind {
            b"iCCP" | b"acTL" => false,
            b"gAMA" => values_near(data, &SRGB_GAMMA),
            b"cHRM" => values_near(data, &SRGB_CHROMATICITIES),
            _ => true,
        };
        if !plain {
            return false;
        }
        if kind == b"IEND" {
            return true;
        }
        at = next;
    }
    false
}

/// sRGB's gamma (1/2.2) and chromaticities (white point, red, green,
/// blue), in units of 1/100000, as the PNG specification gives them.
const SRGB_GAMMA: [u32; 1] = [45_455];
const SRGB_CHROMATICITIES: [u32; 8] = [
    31_270, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
];

/// `data` is big-endian `u32`s each within rounding (±2) of `expected`.
fn values_near(data: &[u8], expected: &[u32]) -> bool {
    data.len() == expected.len() * 4
        && data.chunks_exact(4).zip(expected).all(|(raw, want)| {
            u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]).abs_diff(*want) <= 2
        })
}

fn decode_png(bytes: &[u8]) -> Option<DynamicImage> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    reader.limits(decode_limits());
    let image = reader.decode().ok()?;
    // Lossless WebP holds 8-bit samples only.
    matches!(
        image,
        DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageRgb8(_)
            | DynamicImage::ImageRgba8(_)
    )
    .then_some(image)
}

/// `image` as lossless WebP, if that is smaller than `original_len` and
/// decodes back to exactly the same pixels.
fn smaller_lossless(image: &DynamicImage, original_len: usize) -> Option<Vec<u8>> {
    lossless_webp(image).filter(|webp| webp.len() < original_len)
}

fn lossless_webp(image: &DynamicImage) -> Option<Vec<u8>> {
    let mut webp = Vec::new();
    image
        .write_with_encoder(WebPEncoder::new_lossless(&mut webp))
        .ok()?;
    let mut reader = ImageReader::with_format(Cursor::new(&webp), ImageFormat::WebP);
    reader.limits(decode_limits());
    let back = reader.decode().ok()?;
    (back.to_rgba8() == image.to_rgba8()).then_some(webp)
}

/// A single-frame GIF as smaller lossless WebP, or a large animation as
/// its first frame.
fn gif(bytes: &[u8]) -> Option<(Vec<u8>, MediaDerivation)> {
    let mut decoder = GifDecoder::new(Cursor::new(bytes)).ok()?;
    decoder.set_limits(decode_limits()).ok()?;
    let mut frames = decoder.into_frames();
    // Each frame is the whole canvas as shown at that point.
    let first = DynamicImage::ImageRgba8(frames.next()?.ok()?.into_buffer());
    let animated = frames.next().is_some();
    if !animated {
        return smaller_lossless(&first, bytes.len())
            .map(|webp| (webp, MediaDerivation::LosslessWebp));
    }
    if bytes.len() <= ANIMATION_STILL_BYTES {
        return None;
    }
    lossless_webp(&first).map(|webp| (webp, MediaDerivation::FirstFrameStill))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::codecs::png::PngEncoder;
    use image::{Delay, Frame, ImageEncoder, Rgba, RgbaImage};

    /// A diagram-like image: flat colours and a thin "label" line, which
    /// lossless WebP stores far smaller than PNG does.
    fn diagram(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            if y == height / 2 || x % 50 == 0 {
                Rgba([0, 0, 0, 255])
            } else if x < width / 2 {
                Rgba([200, 30, 30, 255])
            } else {
                Rgba([255, 255, 255, 0])
            }
        })
    }

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        out
    }

    /// Insert a chunk before IDAT.
    fn with_chunk(png: &[u8], kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = png[..33].to_vec();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.finalize().to_be_bytes());
        out.extend_from_slice(&png[33..]);
        out
    }

    fn gif_with(frames: &[RgbaImage]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = GifEncoder::new_with_speed(&mut out, 30);
            encoder.set_repeat(Repeat::Infinite).unwrap();
            for frame in frames {
                encoder
                    .encode_frame(Frame::from_parts(
                        frame.clone(),
                        0,
                        0,
                        Delay::from_numer_denom_ms(100, 1),
                    ))
                    .unwrap();
            }
        }
        out
    }

    fn decoded(bytes: &[u8]) -> RgbaImage {
        image::load_from_memory(bytes).unwrap().to_rgba8()
    }

    /// The first frame as the reader shows it: the whole canvas.
    fn first_gif_frame(bytes: &[u8]) -> RgbaImage {
        let decoder = GifDecoder::new(Cursor::new(bytes)).unwrap();
        decoder.into_frames().next().unwrap().unwrap().into_buffer()
    }

    #[test]
    fn a_png_becomes_smaller_lossless_webp_with_identical_pixels() {
        let source = diagram(400, 300);
        let original = png(&source);
        let packed = pack_image(original.clone(), "image/png");
        assert_eq!(packed.mime, "image/webp");
        assert_eq!(packed.derivation, Some(MediaDerivation::LosslessWebp));
        assert!(packed.bytes.len() < original.len());
        assert_eq!(decoded(&packed.bytes), source);
    }

    #[test]
    fn a_png_lossless_webp_cannot_shrink_is_kept() {
        // Noise: neither format compresses it, and WebP has more overhead.
        let mut seed = 7u32;
        let noise = RgbaImage::from_fn(64, 64, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let [a, b, c, d] = seed.to_be_bytes();
            Rgba([a, b, c, d])
        });
        let original = png(&noise);
        let packed = pack_image(original.clone(), "image/png");
        if packed.derivation.is_none() {
            assert_eq!(packed.bytes, original);
            assert_eq!(packed.mime, "image/png");
        } else {
            assert!(packed.bytes.len() < original.len());
            assert_eq!(decoded(&packed.bytes), noise);
        }
    }

    #[test]
    fn png_colour_management_and_animation_are_never_re_encoded() {
        let original = png(&diagram(400, 300));
        let be =
            |values: &[u32]| -> Vec<u8> { values.iter().flat_map(|v| v.to_be_bytes()).collect() };
        let kept: [(&[u8; 4], Vec<u8>); 4] = [
            (b"iCCP", b"sRGB\0\0profile".to_vec()),
            (b"acTL", be(&[2, 0])),
            (b"gAMA", be(&[100_000])),
            (
                b"cHRM",
                be(&[
                    31_270, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 9_000,
                ]),
            ),
        ];
        for (kind, data) in kept {
            let marked = with_chunk(&original, kind, &data);
            let packed = pack_image(marked.clone(), "image/png");
            assert_eq!(packed.derivation, None, "{}", String::from_utf8_lossy(kind));
            assert_eq!(packed.bytes, marked);
        }
        // Gamma and chromaticities that restate sRGB (as the real Nile basin
        // map's do) change nothing that WebP would show differently.
        let srgb = with_chunk(
            &with_chunk(&original, b"gAMA", &be(&[45_454])),
            b"cHRM",
            &be(&[
                31_270, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
            ]),
        );
        assert_eq!(
            pack_image(srgb, "image/png").derivation,
            Some(MediaDerivation::LosslessWebp)
        );
        // A chunk running past the end of the file is not plain.
        let mut overlong = with_chunk(&original, b"tEXt", b"x");
        overlong[33..37].copy_from_slice(&u32::MAX.to_be_bytes());
        assert_eq!(pack_image(overlong.clone(), "image/png").bytes, overlong);
        // An undecodable or truncated PNG is kept as verified.
        let truncated = original[..original.len() / 2].to_vec();
        assert_eq!(pack_image(truncated.clone(), "image/png").bytes, truncated);
    }

    #[test]
    fn a_still_gif_becomes_lossless_webp() {
        let source = diagram(300, 200);
        let original = gif_with(std::slice::from_ref(&source));
        let packed = pack_image(original.clone(), "image/gif");
        assert_eq!(packed.derivation, Some(MediaDerivation::LosslessWebp));
        assert!(packed.bytes.len() < original.len());
        assert_eq!(decoded(&packed.bytes), first_gif_frame(&original));
    }

    #[test]
    fn a_small_animation_is_kept_whole() {
        let original = gif_with(&[diagram(120, 80), diagram(80, 80).clone(), diagram(120, 80)]);
        assert!(original.len() <= ANIMATION_STILL_BYTES);
        let packed = pack_image(original.clone(), "image/gif");
        assert_eq!(packed.derivation, None);
        assert_eq!(packed.bytes, original);
        assert_eq!(packed.mime, "image/gif");
    }

    #[test]
    fn a_large_animation_is_packed_as_its_first_frame() {
        // Frames of noise make a GIF above the still threshold quickly.
        let mut seed = 1u32;
        let mut frame = || {
            RgbaImage::from_fn(480, 270, |_, _| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let v = (seed >> 24) as u8;
                Rgba([v, v / 2, 255 - v, 255])
            })
        };
        let frames: Vec<RgbaImage> = (0..24).map(|_| frame()).collect();
        let original = gif_with(&frames);
        assert!(original.len() > ANIMATION_STILL_BYTES, "{}", original.len());
        let first_shown = first_gif_frame(&original);
        let packed = pack_image(original.clone(), "image/gif");
        assert_eq!(packed.mime, "image/webp");
        assert_eq!(packed.derivation, Some(MediaDerivation::FirstFrameStill));
        assert_eq!(decoded(&packed.bytes), first_shown);
        assert!(packed.bytes.len() < original.len() / 10);
    }

    #[test]
    fn jpeg_and_webp_are_kept_as_they_are() {
        for mime in ["image/jpeg", "image/webp"] {
            let bytes = b"verified elsewhere".to_vec();
            let packed = pack_image(bytes.clone(), mime);
            assert_eq!(
                (packed.bytes, packed.mime, packed.derivation),
                (bytes, mime, None)
            );
        }
    }
}
