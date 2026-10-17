//! Optional PNG per-chunk CRC guard; not an image decoder or sanitizer.
use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::png_preflight::png_chunk_preflight;
use crate::rendered_local_urls::{
    read_pack_raster_asset_for_source, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};

const SIGNATURE_LEN: usize = 8;

/// Verify each PNG chunk CRC after the structural parser has proved bounds.
/// Passing does not establish valid zlib, pixels, color profiles or metadata.
pub fn verify_png_chunk_crcs(bytes: &[u8]) -> bool {
    if !png_chunk_preflight(bytes) {
        return false;
    }
    let mut offset = SIGNATURE_LEN;
    while offset < bytes.len() {
        let length = u32::from_be_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ]) as usize;
        let data_end = offset + 8 + length;
        let expected = u32::from_be_bytes([
            bytes[data_end],
            bytes[data_end + 1],
            bytes[data_end + 2],
            bytes[data_end + 3],
        ]);
        if crc32(&bytes[offset + 4..data_end]) != expected {
            return false;
        }
        offset = data_end + 4;
    }
    true
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320_u32 & mask);
        }
    }
    !crc
}

/// Opt-in stricter PNG read, preserving source-specific ownership and notices.
pub fn read_source_raster_with_png_crc(
    plan: &VerifiedLocalUrlPlan,
    source_url: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<VerifiedRasterAsset, RasterAssetError> {
    let asset = read_pack_raster_asset_for_source(plan, source_url, registry, pack_id, store)?;
    if asset.mime == "image/png" && !verify_png_chunk_crcs(&asset.bytes) {
        return Err(RasterAssetError::UnsupportedOrMismatchedMime);
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn append_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = png.len();
        png.extend_from_slice(kind);
        png.extend_from_slice(data);
        let checksum = crc32(&png[start..]);
        png.extend_from_slice(&checksum.to_be_bytes());
    }

    fn fixture() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        append_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        append_chunk(&mut png, b"tEXt", b"caption");
        append_chunk(&mut png, b"IDAT", &[0]);
        append_chunk(&mut png, b"IEND", &[]);
        png
    }

    #[test]
    fn validates_crc_and_rejects_each_corrupted_chunk() {
        let original = fixture();
        assert!(verify_png_chunk_crcs(&original));
        for kind in [b"IHDR", b"tEXt", b"IDAT", b"IEND"] {
            let mut altered = original.clone();
            let index = altered
                .windows(4)
                .position(|window| window == kind)
                .unwrap();
            // Flip a structurally permitted IHDR byte; otherwise alter data or CRC.
            let byte_to_corrupt = if kind == b"IHDR" {
                index + 16
            } else {
                index + 4
            };
            altered[byte_to_corrupt] ^= 1;
            assert!(png_chunk_preflight(&altered));
            assert!(!verify_png_chunk_crcs(&altered));
        }
    }

    #[test]
    fn rejects_structural_corruption_and_placeholder_crcs() {
        let original = fixture();
        assert!(!verify_png_chunk_crcs(&original[..30]));
        let mut altered = original.clone();
        altered[29] ^= 1;
        assert!(!verify_png_chunk_crcs(&altered));
        let mut altered = original.clone();
        altered[33] = 0xff;
        assert!(!verify_png_chunk_crcs(&altered));
    }
}
