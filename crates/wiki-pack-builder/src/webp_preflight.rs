//! Additional structural WebP preflight for provisional offline raster reads.
//!
//! This checks RIFF length and first image chunk boundaries. It is not a
//! decoder, active-content sanitizer, trusted manifest, or browser handler.

use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::rendered_local_urls::{
    read_pack_raster_asset, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};

/// Check only the outer RIFF envelope and the first recognized WebP image chunk.
/// Later metadata chunks and image decoding require separate validation.
pub fn webp_riff_preflight(bytes: &[u8]) -> bool {
    if bytes.len() < 22 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WEBP" {
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
        }
        let next = match header_end
            .checked_add(chunk_size)
            .and_then(|end| end.checked_add(chunk_size & 1))
        {
            Some(next) if next <= bytes.len() => next,
            _ => return false,
        };
        offset = next;
        chunks += 1;
    }
    chunks > 0 && offset == bytes.len()
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
