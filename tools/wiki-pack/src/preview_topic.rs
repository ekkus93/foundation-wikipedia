//! Local category-graph preview, not a publisher-verified source importer.
//! The JSON input is explicitly untrusted; no pack is built or installed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use wiki_pack_builder::preview::preview_pinned_selection;
use wiki_pack_builder::selection::{Category, Rules};

const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATEGORIES: usize = 100_000;
const MAX_EDGES: usize = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewInput {
    snapshot_id: String,
    roots: Vec<String>,
    #[serde(default)]
    include: Vec<u64>,
    #[serde(default)]
    exclude: Vec<u64>,
    depth_limit: usize,
    page_limit: usize,
    categories: Vec<PreviewCategory>,
    #[serde(default)]
    redirects: Vec<PreviewRedirect>,
    #[serde(default)]
    disambiguations: Vec<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewCategory {
    name: String,
    #[serde(default)]
    articles: Vec<u64>,
    #[serde(default)]
    children: Vec<String>,
    #[serde(default)]
    administrative: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewRedirect {
    from: u64,
    to: u64,
}

fn safe_category_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

pub fn run(path: &Path, json_output: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot stat preview input: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("preview input must be a regular file, not a symlink".into());
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err("preview input exceeds the 64 MiB limit".into());
    }
    let file = File::open(path).map_err(|error| format!("cannot open preview input: {error}"))?;
    if !file
        .metadata()
        .map_err(|error| format!("cannot inspect open preview input: {error}"))?
        .is_file()
    {
        return Err("opened preview input is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read preview input: {error}"))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("preview input exceeds the 64 MiB limit".into());
    }
    let input: PreviewInput =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid preview JSON: {error}"))?;
    if input.categories.len() > MAX_CATEGORIES
        || input.redirects.len() > MAX_EDGES
        || input.disambiguations.len() > MAX_EDGES
        || input.roots.len() > MAX_CATEGORIES
    {
        return Err("preview source exceeds category or redirect limits".into());
    }
    if input.roots.iter().any(|name| !safe_category_name(name)) {
        return Err("invalid root category name".into());
    }
    let mut graph = BTreeMap::new();
    let mut edges = 0usize;
    for category in input.categories {
        if !safe_category_name(&category.name)
            || category
                .children
                .iter()
                .any(|name| !safe_category_name(name))
            || category.articles.contains(&0)
        {
            return Err("invalid category member or name".into());
        }
        edges = edges
            .checked_add(category.articles.len())
            .and_then(|count| count.checked_add(category.children.len()))
            .filter(|count| *count <= MAX_EDGES)
            .ok_or_else(|| "preview category graph exceeds member limit".to_owned())?;
        if graph
            .insert(
                category.name,
                Category {
                    articles: category.articles,
                    children: category.children,
                    administrative: category.administrative,
                },
            )
            .is_some()
        {
            return Err("duplicate category name".into());
        }
    }
    let mut redirects = BTreeMap::new();
    for redirect in input.redirects {
        if redirect.from == 0
            || redirect.to == 0
            || redirects.insert(redirect.from, redirect.to).is_some()
        {
            return Err("invalid or duplicate redirect source".into());
        }
    }
    if input.disambiguations.contains(&0) {
        return Err("invalid disambiguation page ID".into());
    }
    let rules = Rules {
        roots: input.roots,
        include: input.include,
        exclude: input.exclude,
        depth_limit: input.depth_limit,
        page_limit: input.page_limit,
    };
    let preview = preview_pinned_selection(
        &graph,
        &rules,
        &redirects,
        &input.disambiguations.into_iter().collect::<BTreeSet<_>>(),
        &input.snapshot_id,
    )
    .map_err(|error| format!("topic preview rejected: {error:?}"))?;
    if json_output {
        // Machine-readable pre-download preview for a future GUI/builder
        // integration. No field here confers publisher trust or installability.
        let result = serde_json::json!({
            "schema_version": 1,
            "source": "local-untrusted-category-graph",
            "source_authenticated": false,
            "publisher_signature_verified": false,
            "offline_ready": false,
            "pack_installed": false,
            "snapshot_label": preview.selection.snapshot_id,
            "definition_sha256": preview.selection.definition_sha256,
            "graph_sha256": preview.selection.graph_sha256,
            "selection_sha256": preview.selection.snapshot_selection_sha256,
            "selected_page_ids": preview.selection.page_ids,
            "selected_page_count": preview.selected_page_count,
            "visited_category_count": preview.visited_category_count,
            "skipped_administrative_count": preview.skipped_administrative_count,
            "inspected_article_mentions": preview.inspected_article_mentions,
            "inspected_child_category_mentions": preview.inspected_child_category_mentions,
            "explicit_include_ids": preview.explicit_include_ids,
            "explicit_exclude_ids": preview.explicit_exclude_ids,
            "largest_category": preview.largest_category.as_ref().map(|category| {
                serde_json::json!({
                    "name": category.name,
                    "article_mentions": category.article_mentions,
                    "child_categories": category.child_categories,
                })
            }),
            "warnings": preview.warnings,
        });
        let encoded = serde_json::to_string(&result)
            .map_err(|error| format!("cannot serialize topic preview: {error}"))?;
        println!("{encoded}");
        return Ok(());
    }
    println!("Local topic preview; source publication/authenticity: NOT VERIFIED");
    println!("No media fetched; no pack built, signed, or installed");
    println!(
        "Snapshot label (untrusted): {}",
        preview.selection.snapshot_id
    );
    println!("Selected page IDs: {}", preview.selected_page_count);
    println!("Visited categories: {}", preview.visited_category_count);
    println!(
        "Skipped administrative categories: {}",
        preview.skipped_administrative_count
    );
    println!(
        "Raw article/category mentions inspected: {}/{}",
        preview.inspected_article_mentions, preview.inspected_child_category_mentions
    );
    println!(
        "Definition SHA-256: {}",
        preview.selection.definition_sha256
    );
    println!("Graph SHA-256: {}", preview.selection.graph_sha256);
    println!(
        "Selection SHA-256: {}",
        preview.selection.snapshot_selection_sha256
    );
    for warning in preview.warnings {
        println!("Warning: {warning}");
    }
    Ok(())
}

#[cfg(test)]
mod input_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn test_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fw-topic-preview-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn rejects_directory_before_json_read() {
        let path = test_path();
        fs::create_dir(&path).unwrap();
        let error = run(&path, true).unwrap_err();
        fs::remove_dir(&path).unwrap();
        assert!(error.contains("regular file"), "{error}");
    }

    #[test]
    fn rejects_sparse_oversized_file_before_json_read() {
        let path = test_path();
        let file = File::create(&path).unwrap();
        file.set_len(MAX_INPUT_BYTES + 1).unwrap();
        let error = run(&path, true).unwrap_err();
        fs::remove_file(&path).unwrap();
        assert!(error.contains("64 MiB limit"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_before_json_read() {
        let target = test_path();
        let link = test_path();
        fs::write(&target, b"{}").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let error = run(&link, true).unwrap_err();
        fs::remove_file(&link).unwrap();
        fs::remove_file(&target).unwrap();
        assert!(error.contains("regular file"), "{error}");
    }
}
