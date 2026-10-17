//! Optional bounded JPEG marker/scan preflight; not an entropy decoder.
//! A successful check does not establish safe pixels or authentic provenance.

use wiki_store::media_objects::MediaObjectStore;
use wiki_store::media_ownership::MediaRegistry;

use crate::rendered_local_urls::{
    read_pack_raster_asset_for_source, RasterAssetError, VerifiedLocalUrlPlan, VerifiedRasterAsset,
};

/// Reject malformed marker envelopes and excessive declared frame dimensions.
/// Does not decode Huffman/arithmetic data, ICC profiles or EXIF metadata.
pub fn jpeg_container_preflight(bytes: &[u8]) -> bool {
    if bytes.len() < 20 || bytes.len() > 64 * 1024 * 1024 || !bytes.starts_with(&[0xff, 0xd8]) {
        return false;
    }
    let mut at = 2_usize;
    let mut seen_frame = false;
    let mut seen_scan = false;
    let mut frame_ids = Vec::new();
    for _ in 0..4096 {
        if bytes.get(at) != Some(&0xff) {
            return false;
        }
        while bytes.get(at) == Some(&0xff) {
            at += 1;
        }
        let Some(&marker) = bytes.get(at) else {
            return false;
        };
        at += 1;
        if marker == 0xd9 {
            return seen_frame && seen_scan && at == bytes.len();
        }
        if marker == 0x00 || marker == 0xd8 || marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            return false;
        }
        let Some(header_end) = at.checked_add(2) else {
            return false;
        };
        if header_end > bytes.len() {
            return false;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
        let Some(end) = at.checked_add(length) else {
            return false;
        };
        if length < 2 || end > bytes.len() {
            return false;
        }
        let body = &bytes[header_end..end];
        let is_frame = matches!(
            marker,
            0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
        );
        if is_frame {
            if seen_frame || body.len() < 9 {
                return false;
            }
            let height = u16::from_be_bytes([body[1], body[2]]);
            let width = u16::from_be_bytes([body[3], body[4]]);
            let components = usize::from(body[5]);
            if !(8..=16).contains(&body[0])
                || width == 0
                || height == 0
                || width > 16_384
                || height > 16_384
                || u64::from(width) * u64::from(height) > 64 * 1024 * 1024
                || !(1..=4).contains(&components)
                || body.len() != 6 + 3 * components
            {
                return false;
            }
            for entry in body[6..].chunks_exact(3) {
                if frame_ids.contains(&entry[0]) {
                    return false;
                }
                frame_ids.push(entry[0]);
            }
            seen_frame = true;
        }
        if marker == 0xda {
            if !seen_frame || body.len() < 6 {
                return false;
            }
            let components = usize::from(body[0]);
            if !(1..=4).contains(&components) || body.len() != 4 + 2 * components {
                return false;
            }
            let mut scan_ids = Vec::new();
            for entry in body[1..1 + 2 * components].chunks_exact(2) {
                if !frame_ids.contains(&entry[0]) || scan_ids.contains(&entry[0]) {
                    return false;
                }
                scan_ids.push(entry[0]);
            }
            seen_scan = true;
            at = end;
            let mut has_entropy = false;
            loop {
                let Some(&byte) = bytes.get(at) else {
                    return false;
                };
                if byte != 0xff {
                    has_entropy = true;
                    at += 1;
                    continue;
                }
                let marker_start = at;
                while bytes.get(at) == Some(&0xff) {
                    at += 1;
                }
                let Some(&next) = bytes.get(at) else {
                    return false;
                };
                if next == 0x00 {
                    has_entropy = true;
                    at += 1;
                } else if (0xd0..=0xd7).contains(&next) {
                    at += 1;
                } else {
                    if !has_entropy {
                        return false;
                    }
                    at = marker_start;
                    break;
                }
            }
        } else {
            at = end;
        }
    }
    false
}

/// Source-specific JPEG guard retaining pack ownership and attribution notices.
pub fn read_source_raster_with_jpeg_preflight(
    plan: &VerifiedLocalUrlPlan,
    source_url: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<VerifiedRasterAsset, RasterAssetError> {
    let asset = read_pack_raster_asset_for_source(plan, source_url, registry, pack_id, store)?;
    if asset.mime == "image/jpeg" && !jpeg_container_preflight(&asset.bytes) {
        return Err(RasterAssetError::UnsupportedOrMismatchedMime);
    }
    Ok(asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        // SOF0: 8-bit, one pixel, one component.
        bytes.extend_from_slice(&[0xff, 0xc0, 0, 11, 8, 0, 1, 0, 1, 1, 1, 0x11, 0]);
        // SOS: one component, followed by synthetic entropy and EOI.
        bytes.extend_from_slice(&[
            0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0, 0x11, 0xff, 0, 0x22, 0xff, 0xd9,
        ]);
        bytes
    }

    #[test]
    fn accepts_bounded_marker_and_stuffed_scan_envelope() {
        let original = jpeg();
        assert!(jpeg_container_preflight(&original));
        let mut with_app = original.clone();
        with_app.splice(2..2, [0xff, 0xe1, 0, 4, 0x12, 0x34]);
        assert!(jpeg_container_preflight(&with_app));
        let mut with_restart = original;
        with_restart.splice(29..29, [0xff, 0xd0]);
        assert!(jpeg_container_preflight(&with_restart));
    }

    #[test]
    fn rejects_missing_frame_or_scan_and_bad_dimensions() {
        let original = jpeg();
        let mut no_frame = original.clone();
        no_frame.drain(2..15);
        assert!(!jpeg_container_preflight(&no_frame));
        assert!(!jpeg_container_preflight(&original[..15]));
        let mut zero_width = original.clone();
        zero_width[10] = 0;
        assert!(!jpeg_container_preflight(&zero_width));
        let mut huge = original.clone();
        huge[7..11].copy_from_slice(&[0x40, 0x01, 0x40, 0x01]);
        assert!(!jpeg_container_preflight(&huge));
    }

    #[test]
    fn rejects_scan_components_not_declared_in_frame() {
        let original = jpeg();
        let mut unknown = original.clone();
        unknown[20] = 2;
        assert!(!jpeg_container_preflight(&unknown));

        let mut duplicate_frame = original;
        duplicate_frame[5] = 14;
        duplicate_frame[11] = 2;
        duplicate_frame.splice(15..15, [1, 0x11, 0]);
        assert!(!jpeg_container_preflight(&duplicate_frame));
    }

    #[test]
    fn rejects_truncated_segments_and_entropy_and_trailing_bytes() {
        let original = jpeg();
        let mut truncated = original.clone();
        truncated[5] = 0xff;
        assert!(!jpeg_container_preflight(&truncated));
        let mut no_entropy = original.clone();
        no_entropy.drain(25..29);
        assert!(!jpeg_container_preflight(&no_entropy));
        let mut trailing = original.clone();
        trailing.push(0);
        assert!(!jpeg_container_preflight(&trailing));
        let mut bare_marker = original;
        bare_marker.splice(2..2, [0xff, 0xd0]);
        assert!(!jpeg_container_preflight(&bare_marker));
    }
}
