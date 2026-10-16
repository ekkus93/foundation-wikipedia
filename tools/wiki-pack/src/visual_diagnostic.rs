//! Read-only, bounded CLI adapter for visual URL candidate diagnostics.
//! It never fetches resources, publishes a pack, or asserts media completeness.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use wiki_pack_builder::visual_discovery::{inventory, MAX_MARKUP_BYTES, MAX_REFERENCES};

fn canonical_ascii_json_string(value: &str) -> String {
    // Python's json.dumps(ensure_ascii=True, separators=(",", ":")) behavior.
    let serialized = serde_json::to_string(value).expect("string serialization");
    let mut result = String::with_capacity(serialized.len());
    for character in serialized.chars() {
        if character.is_ascii() {
            result.push(character);
        } else {
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units).iter() {
                use std::fmt::Write as _;
                write!(&mut result, "\\u{unit:04x}").expect("format Unicode escape");
            }
        }
    }
    result
}

fn inventory_fingerprint(
    candidates: &[wiki_pack_builder::visual_discovery::Candidate],
    document_url: &str,
) -> String {
    // v2 binds the canonical sorted candidate list to its exact document URL.
    // This digest identifies a diagnostic observation, NOT downloaded bytes.
    let mut canonical = String::from("[");
    for (index, candidate) in candidates.iter().enumerate() {
        if index > 0 {
            canonical.push(',');
        }
        canonical.push_str("{\"kind\":");
        canonical.push_str(&canonical_ascii_json_string(candidate.kind.as_str()));
        canonical.push_str(",\"url\":");
        canonical.push_str(&canonical_ascii_json_string(&candidate.url));
        canonical.push('}');
    }
    canonical.push(']');
    let mut digest = Sha256::new();
    digest.update(b"foundation-wikipedia/visual-url-candidates/v2\0");
    digest.update(document_url.as_bytes());
    digest.update(b"\0");
    digest.update(canonical.as_bytes());
    format!("{:x}", digest.finalize())
}

fn inspect(path: &Path, document_url: &str) -> Result<Value, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot stat HTML input: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("HTML input must be a regular file, not a symlink".into());
    }
    if metadata.len() > MAX_MARKUP_BYTES as u64 {
        return Err("HTML input exceeds the 64 MiB scanner limit".into());
    }

    let file = File::open(path).map_err(|error| format!("cannot open HTML input: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| format!("cannot inspect open HTML input: {error}"))?
        .is_file()
    {
        return Err("opened HTML input is not a regular file".into());
    }
    // Bound the actual read even if the input grows after metadata inspection.
    let mut reader = file.take(MAX_MARKUP_BYTES as u64 + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read HTML input: {error}"))?;
    if bytes.len() > MAX_MARKUP_BYTES {
        return Err("HTML input exceeds the 64 MiB scanner limit".into());
    }
    let markup = String::from_utf8(bytes).map_err(|_| "HTML input must be UTF-8".to_owned())?;
    let result = inventory(&markup, document_url, MAX_REFERENCES)
        .map_err(|error| format!("visual candidate inventory rejected input: {error:?}"))?;
    let candidates: Vec<Value> = result
        .candidates
        .iter()
        .map(|candidate| json!({"kind":candidate.kind.as_str(), "url":candidate.url}))
        .collect();

    Ok(json!({
        "diagnostic_only": true,
        "inventory_sha256": inventory_fingerprint(&result.candidates, document_url),
        "required_media_verified": result.required_media_verified,
        "css_and_font_dependencies_verified": result.css_and_font_dependencies_verified,
        "document_url": result.document_url,
        "html_base_url": result.html_base_url,
        "observed_references": result.observed_references,
        "unique_candidate_count": candidates.len(),
        "inline_styles": result.inline_styles,
        "inline_style_attributes": result.inline_style_attributes,
        "html_base_href_count": result.html_base_href_count,
        "inline_css_urls": result.inline_css_urls,
        "inline_css_imports": result.inline_css_imports,
        "unresolved_dynamic_surfaces": result.unresolved_dynamic_surfaces,
        "unsupported_schemes": result.unsupported_schemes,
        "missing_sources": result.missing_sources,
        "candidates": candidates
    }))
}

pub fn run(path: &Path, document_url: &str) -> Result<(), String> {
    let report = inspect(path, document_url)?;
    let rendered = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot serialize visual inventory: {error}"))?;
    println!("{rendered}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fixture_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fw-visual-inventory-{}-{}.html",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn candidate_report_is_explicitly_unverified_and_deterministic() {
        let path = fixture_path();
        fs::write(
            &path,
            r#"<img src="a.png"><img src="a.png"><style>.x{background:url(b.svg)}</style>"#,
        )
        .unwrap();
        let report = inspect(&path, "https://en.wikipedia.org/wiki/Test").unwrap();
        fs::remove_file(path).unwrap();

        assert_eq!(report["diagnostic_only"], true);
        assert_eq!(report["required_media_verified"], false);
        assert_eq!(report["css_and_font_dependencies_verified"], false);
        assert_eq!(report["observed_references"], 3);
        assert_eq!(report["unique_candidate_count"], 2);
        assert_eq!(
            report["inventory_sha256"],
            "79d4e3f80db62a1fae08952e007d2c6b8b65d384c7879498233b483642c90860"
        );
        assert_eq!(report["candidates"][0]["kind"], "css_url");
        assert_eq!(
            report["candidates"][0]["url"],
            "https://en.wikipedia.org/wiki/b.svg"
        );
        assert_eq!(report["candidates"][1]["kind"], "image");
    }

    #[test]
    fn invalid_utf8_and_oversized_input_fail_closed() {
        let path = fixture_path();
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(inspect(&path, "https://en.wikipedia.org/wiki/Test").is_err());
        let file = File::create(&path).unwrap();
        file.set_len(MAX_MARKUP_BYTES as u64 + 1).unwrap();
        drop(file);
        assert!(inspect(&path, "https://en.wikipedia.org/wiki/Test").is_err());
        fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_html_input_is_rejected() {
        use std::os::unix::fs::symlink;
        let target = fixture_path();
        let link = fixture_path();
        fs::write(&target, "<img src='a.png'>").unwrap();
        symlink(&target, &link).unwrap();
        assert!(inspect(&link, "https://en.wikipedia.org/wiki/Test").is_err());
        fs::remove_file(link).unwrap();
        fs::remove_file(target).unwrap();
    }
}
