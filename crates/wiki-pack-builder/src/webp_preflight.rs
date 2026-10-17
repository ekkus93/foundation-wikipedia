//! Additional structural WebP preflight for provisional offline raster reads.
//!
//! This checks RIFF length and first image chunk boundaries. It is not a
//! decoder, active-content sanitizer, trusted manifest, or browser handler.

use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::rendered_local_urls::{
    read_pack_raster_asset, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};

/// Check RIFF chunk boundaries, image placement and animation frame envelopes.
/// Image decoding and embedded metadata sanitization require separate validation.
fn valid_animation_frame(frame: &[u8]) -> bool {
    if frame.len() < 30 {
        return false;
    }
    let mut offset = 16;
    let mut seen_alpha = false;
    let mut seen_image = false;
    for _ in 0..2 {
        if offset == frame.len() {
            return seen_image;
        }
        if frame.len() - offset < 8 {
            return false;
        }
        let chunk = &frame[offset..offset + 4];
        let size = u32::from_le_bytes([
            frame[offset + 4],
            frame[offset + 5],
            frame[offset + 6],
            frame[offset + 7],
        ]) as usize;
        let Some(next) = offset
            .checked_add(8)
            .and_then(|end| end.checked_add(size))
            .and_then(|end| end.checked_add(size & 1))
        else {
            return false;
        };
        if next > frame.len() {
            return false;
        }
        if chunk == b"ALPH" && !seen_alpha && !seen_image && size > 0 {
            seen_alpha = true;
        } else if chunk == b"VP8 " && !seen_image && size > 0 {
            seen_image = true;
        } else if chunk == b"VP8L" && !seen_alpha && !seen_image && size > 0 {
            seen_image = true;
        } else {
            return false;
        }
        offset = next;
    }
    offset == frame.len() && seen_image
}

pub fn webp_riff_preflight(bytes: &[u8]) -> bool {
    if bytes.len() < 22
        || bytes.len() > 64 * 1024 * 1024
        || !bytes.starts_with(b"RIFF")
        || &bytes[8..12] != b"WEBP"
    {
        return false;
    }
    let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if riff_size.checked_add(8) != Some(bytes.len()) {
        return false;
    }

    // Scan each declared RIFF chunk without decoding untrusted image content.
    // An incomplete trailing chunk must not slip through a valid first chunk.
    const MAX_CHUNKS: usize = 4096;
    let mut offset = 12;
    let mut chunks = 0;
    let mut extended = false;
    let mut animated = false;
    let mut seen_animation_header = false;
    let mut seen_extended_image = false;
    while offset < bytes.len() {
        let Some(header_end) = offset.checked_add(8) else {
            return false;
        };
        if header_end > bytes.len() || chunks == MAX_CHUNKS {
            return false;
        }
        let chunk = &bytes[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        if chunks == 0 {
            if chunk != b"VP8 " && chunk != b"VP8L" && chunk != b"VP8X" {
                return false;
            }
            if chunk_size == 0 {
                return false;
            }
            extended = chunk == b"VP8X";
        } else if extended {
            if chunk == b"ANIM" {
                if !animated || seen_animation_header || chunk_size != 6 {
                    return false;
                }
                seen_animation_header = true;
            } else if chunk == b"ANMF" {
                if !animated || !seen_animation_header || chunk_size <= 16 {
                    return false;
                }
                seen_extended_image = true;
            } else if chunk == b"VP8 " || chunk == b"VP8L" {
                if animated || chunk_size == 0 || seen_extended_image {
                    return false;
                }
                seen_extended_image = true;
            } else if chunk == b"VP8X" {
                return false;
            }
        } else if chunks > 0
            && (chunk == b"VP8 "
                || chunk == b"VP8L"
                || chunk == b"VP8X"
                || chunk == b"ANIM"
                || chunk == b"ANMF")
        {
            return false;
        }
        let next = match header_end
            .checked_add(chunk_size)
            .and_then(|end| end.checked_add(chunk_size & 1))
        {
            Some(next) if next <= bytes.len() => next,
            _ => return false,
        };
        if chunk == b"ANMF"
            && !valid_animation_frame(&bytes[header_end..header_end + chunk_size])
        {
            return false;
        }
        if chunks == 0 && chunk == b"VP8X" {
            // Extended WebP declares a 24-bit canvas size minus one.
            if chunk_size != 10 {
                return false;
            }
            let ext = &bytes[header_end..header_end + 10];
            if ext[0] & 0xc1 != 0 || ext[1..4] != [0, 0, 0] {
                return false;
            }
            animated = ext[0] & 0x02 != 0;
            let width =
                1 + u32::from(ext[4]) + (u32::from(ext[5]) << 8) + (u32::from(ext[6]) << 16);
            let height =
                1 + u32::from(ext[7]) + (u32::from(ext[8]) << 8) + (u32::from(ext[9]) << 16);
            if width > 16_384
                || height > 16_384
                || u64::from(width) * u64::from(height) > 64 * 1024 * 1024
            {
                return false;
            }
        }
        offset = next;
        chunks += 1;
    }
    chunks > 0 && offset == bytes.len() && (!extended || seen_extended_image)
}

/// Optional stricter read boundary for a future platform scheme adapter.
/// The existing provisional reader remains available and is not silently
/// upgraded; callers must explicitly use this additional WebP gate.
pub fn read_raster_with_webp_preflight(
    plan: &VerifiedLocalUrlPlan,
    local_uri: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<VerifiedRasterAsset, RasterAssetError> {
    let asset = read_pack_raster_asset(plan, local_uri, registry, pack_id, store)?;
    if asset.mime == "image/webp" && !webp_riff_preflight(&asset.bytes) {
        return Err(RasterAssetError::UnsupportedOrMismatchedMime);
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEBP: &[u8] = b"RIFF\x12\x00\x00\x00WEBPVP8L\x05\x00\x00\x00\x2f\x00\x00\x00\x00\x00";

    #[test]
    fn accepts_bounded_first_image_chunk() {
        assert_eq!(WEBP.len(), 26);
        assert!(webp_riff_preflight(WEBP));
    }

    #[test]
    fn checks_all_trailing_riff_chunks_and_padding() {
        let mut with_metadata = WEBP.to_vec();
        with_metadata.extend_from_slice(b"EXIF\x00\x00\x00\x00");
        with_metadata[4..8].copy_from_slice(&26_u32.to_le_bytes());
        assert!(webp_riff_preflight(&with_metadata));

        let mut oversized = with_metadata.clone();
        oversized[30..34].copy_from_slice(&2_u32.to_le_bytes());
        assert!(!webp_riff_preflight(&oversized));

        let mut dangling = WEBP.to_vec();
        dangling.push(0);
        dangling[4..8].copy_from_slice(&19_u32.to_le_bytes());
        assert!(!webp_riff_preflight(&dangling));
    }

    #[test]
    fn checks_extended_canvas_dimensions_and_reserved_header_bytes() {
        let mut ext = b"RIFF\x16\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
        ext.extend_from_slice(&[0; 10]);
        assert!(!webp_riff_preflight(&ext)); // VP8X metadata alone is not an image.
        ext.extend_from_slice(&WEBP[12..]);
        ext[4..8].copy_from_slice(&36_u32.to_le_bytes());
        assert!(webp_riff_preflight(&ext));

        let mut oversized = ext.clone();
        oversized[24] = 0xff;
        oversized[25] = 0xff;
        assert!(!webp_riff_preflight(&oversized));

        let mut bad = ext.clone();
        bad[20] = 0x80;
        assert!(!webp_riff_preflight(&bad));

        bad = ext.clone();
        bad[21] = 1;
        assert!(!webp_riff_preflight(&bad));

        bad = ext.clone();
        bad[16] = 9;
        assert!(!webp_riff_preflight(&bad));
    }

    #[test]
    fn rejects_extended_container_without_image_payload() {
        let mut only_metadata = b"RIFF\x16\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
        only_metadata.extend_from_slice(&[0; 10]);
        assert!(!webp_riff_preflight(&only_metadata));

        only_metadata.extend_from_slice(b"EXIF\x00\x00\x00\x00");
        only_metadata[4..8].copy_from_slice(&30_u32.to_le_bytes());
        assert!(!webp_riff_preflight(&only_metadata));
    }

    #[test]
    fn animated_extended_webp_requires_animation_header_and_frame() {
        let mut sample = b"RIFF\x16\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
        sample.extend_from_slice(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        sample.extend_from_slice(b"ANIM\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00");
        sample.extend_from_slice(b"ANMF\x1e\x00\x00\x00");
        sample.extend_from_slice(&[0; 16]);
        sample.extend_from_slice(&WEBP[12..]);
        let riff_size = (sample.len() - 8) as u32;
        sample[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert!(webp_riff_preflight(&sample));

        let mut no_animation_flag = sample.clone();
        no_animation_flag[20] = 0;
        assert!(!webp_riff_preflight(&no_animation_flag));

        let mut no_animation_header = sample.clone();
        no_animation_header.drain(30..44);
        let riff_size = (no_animation_header.len() - 8) as u32;
        no_animation_header[4..8].copy_from_slice(&riff_size.to_le_bytes());
        assert!(!webp_riff_preflight(&no_animation_header));

        let mut empty_frame = sample.clone();
        empty_frame[48..52].copy_from_slice(&0_u32.to_le_bytes());
        assert!(!webp_riff_preflight(&empty_frame));

        let mut no_frame_image = sample.clone();
        no_frame_image[68..72].copy_from_slice(b"JUNK");
        assert!(!webp_riff_preflight(&no_frame_image));

        let mut oversized_frame_image = sample.clone();
        oversized_frame_image[72..76].copy_from_slice(&100_u32.to_le_bytes());
        assert!(!webp_riff_preflight(&oversized_frame_image));
    }

    #[test]
    fn rejects_duplicate_or_misplaced_image_chunks() {
        let mut simple = WEBP.to_vec();
        simple.extend_from_slice(&WEBP[12..]);
        let simple_size = (simple.len() - 8) as u32;
        simple[4..8].copy_from_slice(&simple_size.to_le_bytes());
        assert!(!webp_riff_preflight(&simple));

        let mut extended = b"RIFF\x16\x00\x00\x00WEBPVP8X\x0a\x00\x00\x00".to_vec();
        extended.extend_from_slice(&[0; 10]);
        extended.extend_from_slice(&WEBP[12..]);
        extended.extend_from_slice(&WEBP[12..]);
        let extended_size = (extended.len() - 8) as u32;
        extended[4..8].copy_from_slice(&extended_size.to_le_bytes());
        assert!(!webp_riff_preflight(&extended));
    }

    #[test]
    fn rejects_truncated_mismatched_or_unrecognized_containers() {
        assert!(!webp_riff_preflight(b"RIFF1234WEBP"));
        assert!(!webp_riff_preflight(&WEBP[..21]));
        let mut bad = WEBP.to_vec();
        bad[4] = 0;
        assert!(!webp_riff_preflight(&bad));
        bad = WEBP.to_vec();
        bad[12..16].copy_from_slice(b"JUNK");
        assert!(!webp_riff_preflight(&bad));
        bad = WEBP.to_vec();
        bad[16] = 99;
        assert!(!webp_riff_preflight(&bad));
        bad = WEBP.to_vec();
        bad[16] = 0;
        assert!(!webp_riff_preflight(&bad));
    }
}
