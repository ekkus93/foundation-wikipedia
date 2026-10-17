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
    let mut baseline_frame = false;
    let mut progressive_dct_frame = false;
    let mut seen_scan = false;
    let mut frame_ids = Vec::new();
    let mut baseline_scanned_ids = Vec::new();
    let mut scanned_frame_ids = Vec::new();
    let mut restart_interval = 0_u16;
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
            return seen_frame
                && seen_scan
                && scanned_frame_ids.len() == frame_ids.len()
                && (!baseline_frame || baseline_scanned_ids.len() == frame_ids.len())
                && at == bytes.len();
        }
        if marker < 0xc0 || marker == 0xd8 || (0xd0..=0xd7).contains(&marker) {
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
                || (marker == 0xc0 && body[0] != 8)
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
                if frame_ids.contains(&entry[0])
                    || !(1..=4).contains(&(entry[1] >> 4))
                    || !(1..=4).contains(&(entry[1] & 0x0f))
                    || entry[2] > 3
                {
                    return false;
                }
                frame_ids.push(entry[0]);
            }
            seen_frame = true;
            baseline_frame = marker == 0xc0;
            progressive_dct_frame = marker == 0xc2;
        }
        if marker == 0xc4 {
            if body.is_empty() {
                return false;
            }
            let mut offset = 0_usize;
            while offset < body.len() {
                if body.len() - offset < 17 {
                    return false;
                }
                let table = body[offset];
                if table >> 4 > 1 || table & 0x0f > 3 {
                    return false;
                }
                let symbols: usize = body[offset + 1..offset + 17]
                    .iter()
                    .map(|&count| usize::from(count))
                    .sum();
                if symbols == 0 || symbols > 256 {
                    return false;
                }
                let mut available_codes = 1_i32;
                for &count in &body[offset + 1..offset + 17] {
                    available_codes = available_codes * 2 - i32::from(count);
                    if available_codes < 0 {
                        return false;
                    }
                }
                offset += 17 + symbols;
                if offset > body.len() {
                    return false;
                }
            }
        }
        if marker == 0xdb {
            if body.is_empty() {
                return false;
            }
            let mut offset = 0_usize;
            while offset < body.len() {
                let table = body[offset];
                let precision = table >> 4;
                if precision > 1 || table & 0x0f > 3 {
                    return false;
                }
                let table_bytes = if precision == 0 { 64 } else { 128 };
                offset += 1 + table_bytes;
                if offset > body.len() {
                    return false;
                }
                let values = &body[offset - table_bytes..offset];
                if (precision == 0 && values.contains(&0))
                    || (precision == 1 && values.chunks_exact(2).any(|pair| pair == [0, 0]))
                {
                    return false;
                }
            }
        }
        if marker == 0xdd {
            if body.len() != 2 {
                return false;
            }
            restart_interval = u16::from_be_bytes([body[0], body[1]]);
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
                if !frame_ids.contains(&entry[0])
                    || scan_ids.contains(&entry[0])
                    || entry[1] >> 4 > 3
                    || entry[1] & 0x0f > 3
                {
                    return false;
                }
                scan_ids.push(entry[0]);
            }
            for id in &scan_ids {
                if !scanned_frame_ids.contains(id) {
                    scanned_frame_ids.push(*id);
                }
            }
            if progressive_dct_frame {
                let spectral_start = body[body.len() - 3];
                let spectral_end = body[body.len() - 2];
                if spectral_end > 63
                    || spectral_start > spectral_end
                    || (spectral_start == 0 && spectral_end != 0)
                    || (spectral_start != 0 && components != 1)
                {
                    return false;
                }
            }
            if baseline_frame {
                if !body.ends_with(&[0, 63, 0]) {
                    return false;
                }
                for id in &scan_ids {
                    if baseline_scanned_ids.contains(id) {
                        return false;
                    }
                    baseline_scanned_ids.push(*id);
                }
            }
            seen_scan = true;
            at = end;
            let mut has_entropy = false;
            let mut entropy_since_restart = false;
            let mut expected_restart = 0xd0_u8;
            loop {
                let Some(&byte) = bytes.get(at) else {
                    return false;
                };
                if byte != 0xff {
                    has_entropy = true;
                    entropy_since_restart = true;
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
                    entropy_since_restart = true;
                    at += 1;
                } else if (0xd0..=0xd7).contains(&next) {
                    if restart_interval == 0 || !entropy_since_restart || next != expected_restart {
                        return false;
                    }
                    expected_restart = if next == 0xd7 { 0xd0 } else { next + 1 };
                    entropy_since_restart = false;
                    at += 1;
                } else {
                    if !has_entropy || !entropy_since_restart {
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
        with_restart.splice(2..2, [0xff, 0xdd, 0, 4, 0, 1]);
        with_restart.splice(34..34, [0xff, 0xd0]);
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
    fn rejects_baseline_precision_and_scan_parameters() {
        let original = jpeg();
        for (offset, value) in [(6, 12), (22, 1), (23, 62), (24, 16)] {
            let mut malformed = original.clone();
            malformed[offset] = value;
            assert!(!jpeg_container_preflight(&malformed), "offset {offset}");
        }
    }

    #[test]
    fn rejects_invalid_frame_sampling_and_table_selectors() {
        let original = jpeg();
        for (offset, value) in [
            (13, 0x01),
            (13, 0x10),
            (13, 0x51),
            (13, 0x15),
            (14, 4),
            (21, 0x40),
            (21, 0x04),
        ] {
            let mut malformed = original.clone();
            malformed[offset] = value;
            assert!(!jpeg_container_preflight(&malformed), "offset {offset}");
        }
    }

    #[test]
    fn validates_quantization_table_envelopes() {
        let original = jpeg();
        let mut dqt = vec![0xff, 0xdb, 0, 67, 0];
        dqt.extend([1_u8; 64]);
        let mut valid = original.clone();
        valid.splice(2..2, dqt.clone());
        assert!(jpeg_container_preflight(&valid));

        for invalid_header in [0x20, 0x04] {
            let mut malformed = valid.clone();
            malformed[6] = invalid_header;
            assert!(!jpeg_container_preflight(&malformed));
        }
        let mut incomplete = original.clone();
        incomplete.splice(2..2, [0xff, 0xdb, 0, 3, 0]);
        assert!(!jpeg_container_preflight(&incomplete));
        let mut empty = original;
        empty.splice(2..2, [0xff, 0xdb, 0, 2]);
        assert!(!jpeg_container_preflight(&empty));
    }

    #[test]
    fn validates_huffman_table_envelopes() {
        let original = jpeg();
        let mut dht = vec![0xff, 0xc4, 0, 20, 0, 1];
        dht.extend([0_u8; 15]);
        dht.push(0x42);
        let mut valid = original.clone();
        valid.splice(2..2, dht);
        assert!(jpeg_container_preflight(&valid));

        for invalid_header in [0x20, 0x04] {
            let mut malformed = valid.clone();
            malformed[6] = invalid_header;
            assert!(!jpeg_container_preflight(&malformed));
        }
        let mut incomplete = original.clone();
        let mut short_table = vec![0xff, 0xc4, 0, 19, 0, 1];
        short_table.extend([0_u8; 15]);
        incomplete.splice(2..2, short_table);
        assert!(!jpeg_container_preflight(&incomplete));
        let mut empty = original;
        empty.splice(2..2, [0xff, 0xc4, 0, 2]);
        assert!(!jpeg_container_preflight(&empty));
    }

    #[test]
    fn rejects_oversubscribed_huffman_codes() {
        let mut dht = vec![0xff, 0xc4, 0, 22, 0, 3];
        dht.extend([0_u8; 15]);
        dht.extend([1, 2, 3]);
        let mut invalid = jpeg();
        invalid.splice(2..2, dht);
        assert!(!jpeg_container_preflight(&invalid));
    }

    #[test]
    fn rejects_zero_quantization_entries() {
        let mut table = vec![0xff, 0xdb, 0, 67, 0];
        table.extend([1_u8; 64]);
        table[5] = 0;
        let mut invalid = jpeg();
        invalid.splice(2..2, table);
        assert!(!jpeg_container_preflight(&invalid));
    }

    #[test]
    fn rejects_unannounced_and_out_of_order_restart_markers() {
        let original = jpeg();
        let mut unannounced = original.clone();
        unannounced.splice(28..28, [0xff, 0xd0]);
        assert!(!jpeg_container_preflight(&unannounced));
        let mut wrong = original;
        wrong.splice(2..2, [0xff, 0xdd, 0, 4, 0, 1]);
        wrong.splice(34..34, [0xff, 0xd1]);
        assert!(!jpeg_container_preflight(&wrong));
    }

    #[test]
    fn baseline_scans_cover_every_component_exactly_once() {
        let original = jpeg();
        let mut two_components = original.clone();
        two_components[5] = 14;
        two_components[11] = 2;
        two_components.splice(15..15, [2, 0x11, 0]);
        assert!(!jpeg_container_preflight(&two_components));

        let mut complete = two_components;
        let at = complete.len() - 2;
        complete.splice(at..at, [0xff, 0xda, 0, 8, 1, 2, 0, 0, 63, 0, 0x33]);
        assert!(jpeg_container_preflight(&complete));

        let mut duplicate = original;
        let at = duplicate.len() - 2;
        duplicate.splice(at..at, [0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0, 0x33]);
        assert!(!jpeg_container_preflight(&duplicate));
    }

    #[test]
    fn progressive_scans_must_cover_every_declared_component() {
        let mut progressive = jpeg();
        progressive[3] = 0xc2; // SOF2 (progressive DCT).
        progressive[5] = 14;
        progressive[11] = 2;
        progressive.splice(15..15, [2, 0x11, 0]);
        progressive[26] = 0; // Progressive DC scan: Ss = Se = 0.
        assert!(!jpeg_container_preflight(&progressive));

        let at = progressive.len() - 2;
        progressive.splice(at..at, [0xff, 0xda, 0, 8, 1, 2, 0, 0, 0, 0, 0x33]);
        assert!(jpeg_container_preflight(&progressive));

        // Multiple progressive scans of the same component are permitted.
        let at = progressive.len() - 2;
        progressive.splice(at..at, [0xff, 0xda, 0, 8, 1, 1, 0, 0, 0, 0, 0x44]);
        assert!(jpeg_container_preflight(&progressive));
    }

    #[test]
    fn progressive_dct_rejects_invalid_spectral_ranges() {
        let mut valid = jpeg();
        valid[3] = 0xc2; // SOF2.
        valid[23] = 0; // DC-only scan.
        assert!(jpeg_container_preflight(&valid));

        for (start, end) in [(0, 1), (4, 3), (1, 64)] {
            let mut invalid = valid.clone();
            invalid[22] = start;
            invalid[23] = end;
            assert!(!jpeg_container_preflight(&invalid));
        }
    }

    #[test]
    fn rejects_reserved_length_bearing_markers() {
        let original = jpeg();
        for marker in [0x02, 0x7f, 0xbf] {
            let mut invalid = original.clone();
            invalid.splice(2..2, [0xff, marker, 0, 4, 0x11, 0x22]);
            assert!(!jpeg_container_preflight(&invalid), "marker {marker:02x}");
        }
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
