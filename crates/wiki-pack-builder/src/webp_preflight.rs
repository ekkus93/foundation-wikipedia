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
    let chunk = &bytes[12..16];
    if chunk != b"VP8 " && chunk != b"VP8L" && chunk != b"VP8X" {
        return false;
    }
    let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let chunk_size = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    let padded = chunk_size.checked_add(chunk_size & 1);
    let first_end = padded.and_then(|size| size.checked_add(20));
    riff_size.checked_add(8) == Some(bytes.len())
        && chunk_size > 0
        && first_end.is_some_and(|end| end <= bytes.len())
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
