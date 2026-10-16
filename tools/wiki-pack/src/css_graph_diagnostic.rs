//! Read-only diagnostic for fully supplied external CSS and @import chains.
//! This does not download, authenticate, install or license any resources.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};
use wiki_pack_builder::visual_discovery::{inventory_supplied_css_graph, MAX_REFERENCES};

const MAX_INPUT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    roots: Vec<String>,
    stylesheets: BTreeMap<String, String>,
}

fn inspect(path: &Path) -> Result<Value, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot stat CSS graph: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("CSS graph input must be a regular file, not a symlink".into());
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err("CSS graph JSON input exceeds the size limit".into());
    }
    let file = File::open(path).map_err(|error| format!("cannot open CSS graph: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| format!("cannot inspect open CSS graph: {error}"))?
        .is_file()
    {
        return Err("opened CSS graph input is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read CSS graph: {error}"))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("CSS graph JSON input exceeds the size limit".into());
    }
    let input: Input =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid CSS graph JSON: {error}"))?;
    let result = inventory_supplied_css_graph(&input.roots, &input.stylesheets, MAX_REFERENCES)
        .map_err(|error| format!("CSS graph rejected: {error:?}"))?;
    let candidates: Vec<Value> = result
        .candidates
        .iter()
        .map(|candidate| json!({"kind": candidate.kind.as_str(), "url": candidate.url}))
        .collect();
    Ok(json!({
        "diagnostic_only": true,
        "stylesheets_inspected": result.stylesheets_inspected,
        "observed_references": result.observed_references,
        "unique_candidate_count": candidates.len(),
        "candidates": candidates,
        "css_and_font_dependencies_verified": result.css_and_font_dependencies_verified,
        "required_media_verified": result.required_media_verified
    }))
}

pub fn run(path: &Path) -> Result<(), String> {
    let report = inspect(path)?;
    let output = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("cannot serialize CSS graph report: {error}"))?;
    println!("{output}");
    Ok(())
}
