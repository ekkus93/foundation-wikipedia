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
    let mut seen_palette = false;
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
            indexed_color = color_type == 3;
            palette_allowed = !matches!(color_type, 0 | 4);
            seen_ihdr = true;
        } else if kind == b"IHDR" {
            return false;
        }
        if kind == b"PLTE" {
            if !palette_allowed
                || seen_palette
                || seen_idat
                || length == 0
                || length > 768
                || length % 3 != 0
            {
                return false;
            }
            seen_palette = true;
        }
        if kind == b"IDAT" {
            if past_idat {
                return false;
            }
            seen_idat = true;
        } else if seen_idat {
            past_idat = true;
        }
        if kind == b"IEND" {
            return length == 0
                && seen_idat
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
