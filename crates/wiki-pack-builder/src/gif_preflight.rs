//! Optional bounded GIF structural guard; not a decoder or sanitizer.

use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::rendered_local_urls::{
    read_pack_raster_asset_for_source, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};

fn skip_blocks(bytes: &[u8], mut at: usize) -> Option<usize> {
    for _ in 0..8192 {
        let size = usize::from(*bytes.get(at)?);
        at = at.checked_add(1)?;
        if size == 0 {
            return Some(at);
        }
        at = at.checked_add(size)?;
        if at > bytes.len() {
            return None;
        }
    }
    None
}

/// Verify bounded GIF frames, color tables and data-block envelopes.
/// Passing does not prove valid LZW codes, pixels or safe metadata.
pub fn gif_container_preflight(bytes: &[u8]) -> bool {
    if bytes.len() < 14
        || bytes.len() > 64 * 1024 * 1024
        || !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
    {
        return false;
    }
    let width = u16::from_le_bytes([bytes[6], bytes[7]]);
    let height = u16::from_le_bytes([bytes[8], bytes[9]]);
    if width == 0
        || height == 0
        || width > 16_384
        || height > 16_384
        || u64::from(width) * u64::from(height) > 64 * 1024 * 1024
    {
        return false;
    }
    let global = bytes[10] & 0x80 != 0;
    let global_entries = if global {
        1_usize << (usize::from(bytes[10] & 7) + 1)
    } else {
        0
    };
    let mut at = 13 + 3 * global_entries;
    let mut transparent_index = None;
    let mut seen_image = false;
    // Bound cumulative decompressed frame area, not just the logical canvas.
    let mut total_frame_pixels = 0_u64;
    for _ in 0..4096 {
        let Some(&marker) = bytes.get(at) else {
            return false;
        };
        match marker {
            0x3b => return seen_image && at + 1 == bytes.len(),
            0x21 => match bytes.get(at + 1) {
                Some(0xf9) => {
                    if bytes.get(at + 2) != Some(&4) || bytes.get(at + 7) != Some(&0) {
                        return false;
                    }
                    let packed = bytes[at + 3];
                    // The top three bits and disposal methods 4..7 are reserved.
                    if packed & 0xe0 != 0 || ((packed >> 2) & 7) > 3 {
                        return false;
                    }
                    transparent_index = if packed & 1 != 0 {
                        Some(usize::from(bytes[at + 6]))
                    } else {
                        None
                    };
                    at += 8;
                }
                Some(0xfe) => {
                    let Some(end) = skip_blocks(bytes, at + 2) else {
                        return false;
                    };
                    at = end;
                }
                Some(0xff) | Some(0x01) => {
                    let expected = if bytes[at + 1] == 0xff { 11 } else { 12 };
                    if bytes.get(at + 2) != Some(&expected) {
                        return false;
                    }
                    let Some(end) = skip_blocks(bytes, at + 2) else {
                        return false;
                    };
                    if bytes[at + 1] == 0x01 {
                        // Plain-text extensions are graphic rendering blocks.
                        transparent_index = None;
                    }
                    at = end;
                }
                _ => return false,
            },
            0x2c => {
                if at + 10 > bytes.len() {
                    return false;
                }
                let x = u16::from_le_bytes([bytes[at + 1], bytes[at + 2]]);
                let y = u16::from_le_bytes([bytes[at + 3], bytes[at + 4]]);
                let w = u16::from_le_bytes([bytes[at + 5], bytes[at + 6]]);
                let h = u16::from_le_bytes([bytes[at + 7], bytes[at + 8]]);
                if w == 0
                    || h == 0
                    || u32::from(x) + u32::from(w) > u32::from(width)
                    || u32::from(y) + u32::from(h) > u32::from(height)
                {
                    return false;
                }
                total_frame_pixels += u64::from(w) * u64::from(h);
                if total_frame_pixels > 64 * 1024 * 1024 {
                    return false;
                }
                let packed = bytes[at + 9];
                let local = packed & 0x80 != 0;
                if !local && !global {
                    return false;
                }
                let entries = if local {
                    1_usize << (usize::from(packed & 7) + 1)
                } else {
                    global_entries
                };
                if transparent_index.take().is_some_and(|index| index >= entries) {
                    return false;
                }
                at += 10;
                if local {
                    at += 3 * entries;
                }
                let Some(&code_size) = bytes.get(at) else {
                    return false;
                };
                if !(2..=8).contains(&code_size) || bytes.get(at + 1) == Some(&0) {
                    return false;
                }
                let Some(end) = skip_blocks(bytes, at + 1) else {
                    return false;
                };
                at = end;
                seen_image = true;
            }
            _ => return false,
        }
    }
    false
}

/// Opt-in source-specific read, retaining pack ownership and media notices.
pub fn read_source_raster_with_gif_preflight(
    plan: &VerifiedLocalUrlPlan,
    source_url: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<VerifiedRasterAsset, RasterAssetError> {
    let asset = read_pack_raster_asset_for_source(plan, source_url, registry, pack_id, store)?;
    if asset.mime == "image/gif" && !gif_container_preflight(&asset.bytes) {
        return Err(RasterAssetError::UnsupportedOrMismatchedMime);
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif() -> Vec<u8> {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
        bytes.extend_from_slice(&[0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        bytes.extend_from_slice(&[2, 2, 0x4c, 1, 0, 0x3b]);
        bytes
    }

    #[test]
    fn accepts_frame_and_graphics_control_extension() {
        let original = gif();
        assert!(gif_container_preflight(&original));
        let mut extended = original.clone();
        extended.splice(19..19, [0x21, 0xf9, 4, 0, 0, 0, 0, 0]);
        assert!(gif_container_preflight(&extended));
    }

    #[test]
    fn validates_graphics_control_flags_and_transparent_palette_index() {
        let original = gif();
        let mut valid = original.clone();
        valid.splice(19..19, [0x21, 0xf9, 4, 0x01, 0, 0, 1, 0]);
        assert!(gif_container_preflight(&valid));

        let mut reserved = valid.clone();
        reserved[22] = 0x21;
        assert!(!gif_container_preflight(&reserved));

        let mut disposal = valid.clone();
        disposal[22] = 0x11;
        assert!(!gif_container_preflight(&disposal));

        let mut invalid_index = valid.clone();
        invalid_index[25] = 2;
        assert!(!gif_container_preflight(&invalid_index));

        // A frame-local palette overrides the two-entry global palette.
        let mut local = valid;
        local[36] = 0x81;
        local.splice(37..37, [0; 12]);
        local[25] = 3;
        assert!(gif_container_preflight(&local));
    }

    #[test]
    fn rejects_excessive_total_frame_area() {
        let mut single = gif();
        single[6..10].copy_from_slice(&[0, 32, 0, 32]);
        single[24..28].copy_from_slice(&[0, 32, 0, 32]);
        assert!(gif_container_preflight(&single));

        let mut repeated = single.clone();
        repeated.splice(
            single.len() - 1..single.len() - 1,
            single[19..single.len() - 1].iter().copied(),
        );
        assert!(!gif_container_preflight(&repeated));
    }

    #[test]
    fn rejects_unknown_or_malformed_extension_headers() {
        let original = gif();
        let mut with_comment = original.clone();
        with_comment.splice(19..19, [0x21, 0xfe, 3, b'a', b'b', b'c', 0]);
        assert!(gif_container_preflight(&with_comment));

        let mut unknown = original.clone();
        unknown.splice(19..19, [0x21, 0x00, 0]);
        assert!(!gif_container_preflight(&unknown));

        let mut malformed_app = original.clone();
        malformed_app.splice(19..19, [0x21, 0xff, 1, b'a', 0]);
        assert!(!gif_container_preflight(&malformed_app));
    }

    #[test]
    fn rejects_truncation_bad_blocks_and_dimensions() {
        let original = gif();
        assert!(!gif_container_preflight(&original[..34]));
        let mut bad = original.clone();
        bad[30] = 250;
        assert!(!gif_container_preflight(&bad));
        bad = original.clone();
        bad[6] = 0;
        assert!(!gif_container_preflight(&bad));
        bad = original.clone();
        bad.push(0);
        assert!(!gif_container_preflight(&bad));
    }
}
