//! Optional bounded PNG structural preflight for offline raster candidates.
//! This checks PNG chunk envelopes and IHDR resource limits only. It does not
//! verify CRCs, decompress IDAT, sanitize embedded metadata or decode an image.

use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::rendered_local_urls::{
    read_pack_raster_asset_for_source, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Refuse impossible/truncated PNG chunk envelopes and decompression-size
/// hazards before a future platform decoder sees untrusted image bytes.
/// A passing result MUST NOT be treated as valid pixel data or a safe render.
pub fn png_chunk_preflight(bytes: &[u8]) -> bool {
    const MAX_PNG_BYTES: usize = 64 * 1024 * 1024;
    const MAX_CHUNKS: usize = 4096;
    const MAX_DIMENSION: u32 = 16_384;
    const MAX_PIXELS: u64 = 64 * 1024 * 1024;

    if bytes.len() < 58 || bytes.len() > MAX_PNG_BYTES || !bytes.starts_with(PNG_SIGNATURE) {
        return false;
    }
    let mut offset = PNG_SIGNATURE.len();
    let mut seen_ihdr = false;
    let mut seen_idat = false;
    let mut seen_idat_data = false;
    let mut seen_palette = false;
    let mut palette_entries = 0_usize;
    let mut png_color_type = 0_u8;
    let mut png_bit_depth = 0_u8;
    let mut seen_transparency = false;
    let mut seen_srgb = false;
    let mut max_palette_entries = 256_usize;
    let mut indexed_color = false;
    let mut palette_allowed = true;
    let mut past_idat = false;
    for _ in 0..MAX_CHUNKS {
        let Some(header_end) = offset.checked_add(8) else {
            return false;
        };
        if header_end > bytes.len() {
            return false;
        }
        let length = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let kind = &bytes[offset + 4..header_end];
        if !kind.iter().all(u8::is_ascii_alphabetic) {
            return false;
        }
        // The PNG reserved bit must be uppercase; unknown critical chunks
        // cannot be safely interpreted by a future renderer.
        let known_critical =
            kind == b"IHDR" || kind == b"PLTE" || kind == b"IDAT" || kind == b"IEND";
        if kind[2].is_ascii_lowercase() || (kind[0].is_ascii_uppercase() && !known_critical) {
            return false;
        }
        let Some(next) = header_end
            .checked_add(length)
            .and_then(|end| end.checked_add(4))
        else {
            return false;
        };
        if next > bytes.len() {
            return false;
        }
        if !seen_ihdr {
            if kind != b"IHDR" || length != 13 {
                return false;
            }
            let width = u32::from_be_bytes([
                bytes[header_end],
                bytes[header_end + 1],
                bytes[header_end + 2],
                bytes[header_end + 3],
            ]);
            let height = u32::from_be_bytes([
                bytes[header_end + 4],
                bytes[header_end + 5],
                bytes[header_end + 6],
                bytes[header_end + 7],
            ]);
            let bit_depth = bytes[header_end + 8];
            let color_type = bytes[header_end + 9];
            let supported_depth = match color_type {
                0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
                2 | 4 | 6 => matches!(bit_depth, 8 | 16),
                3 => matches!(bit_depth, 1 | 2 | 4 | 8),
                _ => false,
            };
            if !supported_depth
                || width == 0
                || height == 0
                || width > MAX_DIMENSION
                || height > MAX_DIMENSION
                || u64::from(width) * u64::from(height) > MAX_PIXELS
                || bytes[header_end + 10] != 0
                || bytes[header_end + 11] != 0
                || bytes[header_end + 12] > 1
            {
                return false;
            }
            png_color_type = color_type;
            png_bit_depth = bit_depth;
            indexed_color = color_type == 3;
            if indexed_color {
                max_palette_entries = 1_usize << bit_depth;
            }
            palette_allowed = !matches!(color_type, 0 | 4);
            seen_ihdr = true;
        } else if kind == b"IHDR" {
            return false;
        }
        if kind == b"PLTE" {
            if !palette_allowed
                || seen_palette
                || seen_transparency
                || seen_idat
                || length == 0
                || length > 768
                || length % 3 != 0
                || length / 3 > max_palette_entries
            {
                return false;
            }
            seen_palette = true;
            palette_entries = length / 3;
        }
        if kind == b"tRNS" {
            let invalid = match png_color_type {
                0 => {
                    if length != 2 {
                        true
                    } else {
                        let sample = u16::from_be_bytes([bytes[header_end], bytes[header_end + 1]]);
                        png_bit_depth < 16 && sample >= (1_u16 << png_bit_depth)
                    }
                }
                2 => {
                    length != 6
                        || (png_bit_depth == 8
                            && bytes[header_end..header_end + 6]
                                .chunks_exact(2)
                                .any(|sample| sample[0] != 0))
                }
                3 => !seen_palette || length == 0 || length > palette_entries,
                _ => true,
            };
            if seen_transparency || seen_idat || invalid {
                return false;
            }
            seen_transparency = true;
        }
        if kind == b"sRGB" {
            if seen_srgb || seen_palette || seen_idat || length != 1 || bytes[header_end] > 3 {
                return false;
            }
            seen_srgb = true;
        }
        if kind == b"IDAT" {
            if past_idat {
                return false;
            }
            seen_idat = true;
            seen_idat_data |= length > 0;
        } else if seen_idat {
            past_idat = true;
        }
        if kind == b"IEND" {
            return length == 0
                && seen_idat
                && seen_idat_data
                && (!indexed_color || seen_palette)
                && next == bytes.len();
        }
        offset = next;
    }
    false
}

/// Optional stricter read boundary: require a source-specific notice and
/// chunk-level preflight for PNG. Other MIME types retain existing policy.
pub fn read_source_raster_with_png_preflight(
    plan: &VerifiedLocalUrlPlan,
    source_url: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<VerifiedRasterAsset, RasterAssetError> {
    let asset = read_pack_raster_asset_for_source(plan, source_url, registry, pack_id, store)?;
    if asset.mime == "image/png" && !png_chunk_preflight(&asset.bytes) {
        return Err(RasterAssetError::UnsupportedOrMismatchedMime);
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Dummy CRC fields keep these as structural fixtures, not decoded images.
    fn add_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        png.extend_from_slice(&[0; 4]);
    }

    fn sample_png() -> Vec<u8> {
        let mut png = PNG_SIGNATURE.to_vec();
        add_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        add_chunk(&mut png, b"IDAT", &[0]);
        add_chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn accepts_bounded_png_chunk_sequence_without_claiming_crc_or_decode() {
        let png = sample_png();
        assert!(png_chunk_preflight(&png));
        let mut with_text = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut with_text,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0],
        );
        add_chunk(&mut with_text, b"tEXt", b"caption");
        add_chunk(&mut with_text, b"IDAT", &[0]);
        add_chunk(&mut with_text, b"IEND", &[]);
        assert!(png_chunk_preflight(&with_text));
    }

    #[test]
    fn rejects_unknown_critical_chunks_and_reserved_bit_violations() {
        let original = sample_png();
        let mut unknown = original.clone();
        let mut chunk = Vec::new();
        add_chunk(&mut chunk, b"ABCD", &[]);
        unknown.splice(33..33, chunk);
        assert!(!png_chunk_preflight(&unknown));

        let mut reserved = original.clone();
        let mut chunk = Vec::new();
        add_chunk(&mut chunk, b"abcc", &[]);
        reserved.splice(33..33, chunk);
        assert!(!png_chunk_preflight(&reserved));

        let mut ancillary = original;
        let mut chunk = Vec::new();
        add_chunk(&mut chunk, b"abCd", &[]);
        ancillary.splice(33..33, chunk);
        assert!(png_chunk_preflight(&ancillary));
    }

    #[test]
    fn enforces_valid_color_depth_and_palette_ordering() {
        let mut png = sample_png();
        png[24] = 4;
        assert!(!png_chunk_preflight(&png));
        png = sample_png();
        png[25] = 5;
        assert!(!png_chunk_preflight(&png));

        let mut indexed = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut indexed,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0],
        );
        add_chunk(&mut indexed, b"IDAT", &[0]);
        add_chunk(&mut indexed, b"IEND", &[]);
        assert!(!png_chunk_preflight(&indexed));

        let mut indexed_with_palette = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut indexed_with_palette,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0],
        );
        add_chunk(&mut indexed_with_palette, b"PLTE", &[255, 0, 0]);
        add_chunk(&mut indexed_with_palette, b"IDAT", &[0]);
        add_chunk(&mut indexed_with_palette, b"IEND", &[]);
        assert!(png_chunk_preflight(&indexed_with_palette));

        let mut separated_idat = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut separated_idat,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0],
        );
        add_chunk(&mut separated_idat, b"IDAT", &[0]);
        add_chunk(&mut separated_idat, b"tEXt", b"caption");
        add_chunk(&mut separated_idat, b"IDAT", &[0]);
        add_chunk(&mut separated_idat, b"IEND", &[]);
        assert!(!png_chunk_preflight(&separated_idat));
    }

    #[test]
    fn truecolor_palette_must_precede_transparency_chunk() {
        let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0];
        let mut valid = PNG_SIGNATURE.to_vec();
        add_chunk(&mut valid, b"IHDR", &ihdr);
        add_chunk(&mut valid, b"PLTE", &[255, 0, 0]);
        add_chunk(&mut valid, b"tRNS", &[0, 255, 0, 0, 0, 0]);
        add_chunk(&mut valid, b"IDAT", &[0]);
        add_chunk(&mut valid, b"IEND", &[]);
        assert!(png_chunk_preflight(&valid));

        let mut invalid = PNG_SIGNATURE.to_vec();
        add_chunk(&mut invalid, b"IHDR", &ihdr);
        add_chunk(&mut invalid, b"tRNS", &[0, 255, 0, 0, 0, 0]);
        add_chunk(&mut invalid, b"PLTE", &[255, 0, 0]);
        add_chunk(&mut invalid, b"IDAT", &[0]);
        add_chunk(&mut invalid, b"IEND", &[]);
        assert!(!png_chunk_preflight(&invalid));
    }

    #[test]
    fn srgb_chunk_requires_valid_intent_unique_placement_before_palette_and_idat() {
        let mut valid = sample_png();
        let mut srgb = Vec::new();
        add_chunk(&mut srgb, b"sRGB", &[0]);
        valid.splice(33..33, srgb.clone());
        assert!(png_chunk_preflight(&valid));

        let mut duplicate = valid.clone();
        duplicate.splice(33..33, srgb);
        assert!(!png_chunk_preflight(&duplicate));

        for payload in [&[][..], &[4_u8][..], &[0_u8, 1][..]] {
            let mut invalid = sample_png();
            let mut chunk = Vec::new();
            add_chunk(&mut chunk, b"sRGB", payload);
            invalid.splice(33..33, chunk);
            assert!(!png_chunk_preflight(&invalid));
        }

        let mut after_idat = sample_png();
        let iend_offset = after_idat.windows(4).position(|w| w == b"IEND").unwrap() - 4;
        let mut chunk = Vec::new();
        add_chunk(&mut chunk, b"sRGB", &[2]);
        after_idat.splice(iend_offset..iend_offset, chunk);
        assert!(!png_chunk_preflight(&after_idat));

        let mut after_palette = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut after_palette,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0],
        );
        add_chunk(&mut after_palette, b"PLTE", &[255, 0, 0]);
        add_chunk(&mut after_palette, b"sRGB", &[1]);
        add_chunk(&mut after_palette, b"IDAT", &[0]);
        add_chunk(&mut after_palette, b"IEND", &[]);
        assert!(!png_chunk_preflight(&after_palette));
    }

    #[test]
    fn requires_nonempty_idat_data_but_allows_empty_split_chunks() {
        let mut png = PNG_SIGNATURE.to_vec();
        add_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        add_chunk(&mut png, b"tEXt", b"metadata");
        add_chunk(&mut png, b"IDAT", &[]);
        add_chunk(&mut png, b"IEND", &[]);
        assert!(!png_chunk_preflight(&png));

        let mut split = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut split,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0],
        );
        add_chunk(&mut split, b"IDAT", &[]);
        add_chunk(&mut split, b"IDAT", &[0]);
        add_chunk(&mut split, b"IEND", &[]);
        assert!(png_chunk_preflight(&split));
    }

    #[test]
    fn indexed_palette_cannot_exceed_bit_depth_capacity() {
        let mut png = PNG_SIGNATURE.to_vec();
        add_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 1, 3, 0, 0, 0]);
        add_chunk(&mut png, b"PLTE", &[255, 0, 0, 0, 0, 255]);
        add_chunk(&mut png, b"IDAT", &[0]);
        add_chunk(&mut png, b"IEND", &[]);
        assert!(png_chunk_preflight(&png));

        let mut too_many = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut too_many,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 1, 3, 0, 0, 0],
        );
        add_chunk(&mut too_many, b"PLTE", &[255, 0, 0, 0, 0, 255, 0, 255, 0]);
        add_chunk(&mut too_many, b"IDAT", &[0]);
        add_chunk(&mut too_many, b"IEND", &[]);
        assert!(!png_chunk_preflight(&too_many));
    }

    #[test]
    fn validates_transparency_chunk_shape_and_order() {
        let mut indexed = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut indexed,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0],
        );
        add_chunk(&mut indexed, b"PLTE", &[255, 0, 0]);
        add_chunk(&mut indexed, b"tRNS", &[128]);
        add_chunk(&mut indexed, b"IDAT", &[0]);
        add_chunk(&mut indexed, b"IEND", &[]);
        assert!(png_chunk_preflight(&indexed));

        let mut too_many = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut too_many,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0],
        );
        add_chunk(&mut too_many, b"PLTE", &[255, 0, 0]);
        add_chunk(&mut too_many, b"tRNS", &[128, 255]);
        add_chunk(&mut too_many, b"IDAT", &[0]);
        add_chunk(&mut too_many, b"IEND", &[]);
        assert!(!png_chunk_preflight(&too_many));

        let mut duplicate = indexed.clone();
        let mut chunk = Vec::new();
        add_chunk(&mut chunk, b"tRNS", &[255]);
        let idat_offset = duplicate.windows(4).position(|w| w == b"IDAT").unwrap() - 4;
        duplicate.splice(idat_offset..idat_offset, chunk);
        assert!(!png_chunk_preflight(&duplicate));

        let mut after_idat = indexed.clone();
        let transparency_offset = after_idat.windows(4).position(|w| w == b"tRNS").unwrap() - 4;
        let chunk: Vec<u8> = after_idat
            .drain(transparency_offset..transparency_offset + 13)
            .collect();
        let iend_offset = after_idat.windows(4).position(|w| w == b"IEND").unwrap() - 4;
        after_idat.splice(iend_offset..iend_offset, chunk);
        assert!(!png_chunk_preflight(&after_idat));

        let mut alpha = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut alpha,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0],
        );
        add_chunk(&mut alpha, b"tRNS", &[0]);
        add_chunk(&mut alpha, b"IDAT", &[0]);
        add_chunk(&mut alpha, b"IEND", &[]);
        assert!(!png_chunk_preflight(&alpha));

        let mut grayscale = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut grayscale,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0],
        );
        add_chunk(&mut grayscale, b"tRNS", &[0, 0]);
        add_chunk(&mut grayscale, b"IDAT", &[0]);
        add_chunk(&mut grayscale, b"IEND", &[]);
        assert!(png_chunk_preflight(&grayscale));
    }

    #[test]
    fn rejects_out_of_range_grayscale_and_rgb_transparency_samples() {
        for (depth, sample, expected) in [
            (1_u8, 1_u16, true),
            (1, 2, false),
            (8, 255, true),
            (8, 256, false),
            (16, 65535, true),
        ] {
            let mut png = PNG_SIGNATURE.to_vec();
            add_chunk(
                &mut png,
                b"IHDR",
                &[0, 0, 0, 1, 0, 0, 0, 1, depth, 0, 0, 0, 0],
            );
            add_chunk(&mut png, b"tRNS", &sample.to_be_bytes());
            add_chunk(&mut png, b"IDAT", &[0]);
            add_chunk(&mut png, b"IEND", &[]);
            assert_eq!(png_chunk_preflight(&png), expected, "gray depth {depth}");
        }
        for high_byte in [0_u8, 1] {
            let mut png = PNG_SIGNATURE.to_vec();
            add_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
            add_chunk(&mut png, b"tRNS", &[high_byte, 0, 0, 1, 0, 2]);
            add_chunk(&mut png, b"IDAT", &[0]);
            add_chunk(&mut png, b"IEND", &[]);
            assert_eq!(png_chunk_preflight(&png), high_byte == 0);
        }
    }

    #[test]
    fn rejects_invalid_png_structure_and_resource_budgets() {
        let png = sample_png();
        assert!(!png_chunk_preflight(&png[..30]));
        let mut wrong_signature = png.clone();
        wrong_signature[0] = 0;
        assert!(!png_chunk_preflight(&wrong_signature));
        let mut wrong_width = png.clone();
        wrong_width[16..20].copy_from_slice(&0_u32.to_be_bytes());
        assert!(!png_chunk_preflight(&wrong_width));
        let mut oversized = png.clone();
        oversized[16..20].copy_from_slice(&16_385_u32.to_be_bytes());
        assert!(!png_chunk_preflight(&oversized));
        let mut impossible_chunk = png.clone();
        impossible_chunk[33..37].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(!png_chunk_preflight(&impossible_chunk));
        let mut trailing = png.clone();
        trailing.push(0);
        assert!(!png_chunk_preflight(&trailing));
        let mut no_idat = PNG_SIGNATURE.to_vec();
        add_chunk(
            &mut no_idat,
            b"IHDR",
            &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0],
        );
        add_chunk(&mut no_idat, b"IEND", &[]);
        assert!(!png_chunk_preflight(&no_idat));
    }
}
