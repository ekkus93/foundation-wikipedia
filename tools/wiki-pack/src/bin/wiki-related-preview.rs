//! Provisional local linked-page preview; input does not establish Wikimedia trust.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use wiki_pack_builder::related::expand_related_links;

const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LINKS: usize = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    seeds: Vec<u64>,
    #[serde(default)]
    exclude: Vec<u64>,
    max_hops: usize,
    max_new_pages: usize,
    max_edge_visits: usize,
    links: Vec<Link>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Link {
    from: u64,
    to: u64,
}

fn run(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        for ancestor in parent.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let metadata = fs::symlink_metadata(ancestor)
                .map_err(|error| format!("invalid input parent: {error}"))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err("input path has an unsafe parent".into());
            }
        }
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot stat input: {error}"))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_INPUT_BYTES {
        return Err("input must be a regular file at most 8 MiB".into());
    }
    let file = File::open(path).map_err(|error| format!("cannot open input: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| format!("cannot inspect input: {error}"))?
        .is_file()
    {
        return Err("opened input is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read input: {error}"))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("input exceeds 8 MiB".into());
    }
    let input_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let input: Input =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid JSON: {error}"))?;
    if input.links.len() > MAX_LINKS
        || input.seeds.len().saturating_add(input.exclude.len()) > MAX_LINKS
    {
        return Err("related input exceeds member budget".into());
    }
    let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for link in input.links {
        if link.from == 0 || link.to == 0 {
            return Err("links require nonzero canonical page IDs".into());
        }
        adjacency.entry(link.from).or_default().push(link.to);
    }
    let excluded: BTreeSet<u64> = input.exclude.into_iter().collect();
    let initial = input
        .seeds
        .iter()
        .copied()
        .filter(|id| !excluded.contains(id))
        .collect::<BTreeSet<_>>()
        .len();
    let expanded = expand_related_links(
        &input.seeds,
        &adjacency,
        &excluded,
        input.max_hops,
        input.max_new_pages,
        input.max_edge_visits,
    )
    .map_err(|error| format!("related preview rejected: {error:?}"))?;
    let report = serde_json::json!({
        "schema_version": 1,
        "source": "local-untrusted-related-links",
        "input_sha256": input_sha256,
        "source_authenticated": false,
        "offline_ready": false,
        "pack_installed": false,
        "selected_page_count": expanded.len(),
        "additional_page_count": expanded.len() - initial,
        "selected_page_ids": expanded,
    });
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        eprintln!("Usage: wiki-related-preview <local-related-links.json>");
        std::process::exit(2);
    }
    if let Err(error) = run(Path::new(&args[0])) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
