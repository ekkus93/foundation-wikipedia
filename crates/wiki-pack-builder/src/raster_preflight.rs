//! Shared opt-in raster structural validation, not a decoder or sanitizer.

use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::gif_preflight::gif_container_preflight;
use crate::jpeg_preflight::jpeg_container_preflight;
use crate::png_crc::verify_png_chunk_crcs;
use crate::rendered_local_urls::{
    read_pack_raster_asset_for_source, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};
use crate::webp_preflight::webp_riff_preflight;

/// Fail closed for unsupported MIME and malformed raster envelopes.
pub fn raster_structural_preflight(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => verify_png_chunk_crcs(bytes),
        "image/jpeg" => jpeg_container_preflight(bytes),
        "image/gif" => gif_container_preflight(bytes),
        "image/webp" => webp_riff_preflight(bytes),
        _ => false,
    }
}

/// Require source-specific attribution and pack ownership before returning
/// structurally checked raster bytes. This is not an origin-restricted handler.
pub fn read_source_raster_with_structural_preflight(
    plan: &VerifiedLocalUrlPlan,
    source_url: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<VerifiedRasterAsset, RasterAssetError> {
    let asset = read_pack_raster_asset_for_source(plan, source_url, registry, pack_id, store)?;
    if !raster_structural_preflight(&asset.mime, &asset.bytes) {
        return Err(RasterAssetError::UnsupportedOrMismatchedMime);
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_and_unsupported_raster_payloads() {
        assert!(!raster_structural_preflight(
            "image/jpeg",
            &[0xff, 0xd8, 0xff, 0xd9]
        ));
        assert!(!raster_structural_preflight(
            "image/png",
            b"\x89PNG\r\n\x1a\n"
        ));
        assert!(!raster_structural_preflight("image/gif", b"GIF89a"));
        assert!(!raster_structural_preflight("image/webp", b"RIFF"));
        assert!(!raster_structural_preflight("image/svg+xml", b"<svg/>"));
    }
}
