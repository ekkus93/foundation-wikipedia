//! Optional bounded GIF structural guard; not a decoder or sanitizer.

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
    let mut at = 13;
    if global {
        at += 3 * (1_usize << (usize::from(bytes[10] & 7) + 1));
    }
    let mut seen_image = false;
    for _ in 0..4096 {
        let Some(&marker) = bytes.get(at) else {
            return false;
        };
        match marker {
            0x3b => return seen_image && at + 1 == bytes.len(),
            0x21 => {
                if bytes.get(at + 1) == Some(&0xf9) {
                    if bytes.get(at + 2) != Some(&4) || bytes.get(at + 7) != Some(&0) {
                        return false;
                    }
                    at += 8;
                } else {
                    let Some(end) = skip_blocks(bytes, at + 2) else {
                        return false;
                    };
                    at = end;
                }
            }
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
                let packed = bytes[at + 9];
                let local = packed & 0x80 != 0;
                if !local && !global {
                    return false;
                }
                at += 10;
                if local {
                    at += 3 * (1_usize << (usize::from(packed & 7) + 1));
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
    fn rejects_truncation_bad_blocks_and_dimensions() {
        let original = gif();
        assert!(!gif_container_preflight(&original[..34]));
        let mut bad = original.clone();
        bad[31] = 250;
        assert!(!gif_container_preflight(&bad));
        bad = original.clone();
        bad[6] = 0;
        assert!(!gif_container_preflight(&bad));
        bad = original.clone();
        bad.push(0);
        assert!(!gif_container_preflight(&bad));
    }
}
