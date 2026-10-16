//! Local category-graph preview, not a publisher-verified source importer.
//! The JSON input is explicitly untrusted; no pack is built or installed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use wiki_pack_builder::preview::preview_pinned_selection;
use wiki_pack_builder::related::expand_related_links;
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
    #[serde(default)]
    related: Option<RelatedInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelatedInput {
    max_hops: usize,
    max_new_pages: usize,
    max_edge_visits: usize,
    links: Vec<RelatedLink>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelatedLink {
    from: u64,
    to: u64,
}

struct RelatedSelection {
    page_ids: Vec<u64>,
    selection_sha256: String,
    added_count: usize,
    link_mentions: usize,
}

fn canonical_related_id(mut id: u64, redirects: &BTreeMap<u64, u64>) -> Result<u64, String> {
    let mut seen = BTreeSet::new();
    while let Some(&next) = redirects.get(&id) {
        if !seen.insert(id) || seen.len() > 4096 {
            return Err("related link redirect cycle or excessive depth".into());
        }
        id = next;
    }
    if id == 0 {
        return Err("related links require nonzero page IDs".into());
    }
    Ok(id)
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
    if let Some(parent) = path.parent() {
        for ancestor in parent.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let info = fs::symlink_metadata(ancestor)
                .map_err(|error| format!("cannot inspect preview input path: {error}"))?;
            if info.file_type().is_symlink() {
                return Err("preview input path contains a symlink".into());
            }
        }
    }
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
    let input_sha256 = format!("{:x}", Sha256::digest(&bytes));
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
    let related = input.related;
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
    let related_report = if let Some(related) = related {
        if related.links.len() > MAX_EDGES {
            return Err("related links exceed the member limit".into());
        }
        let excluded: BTreeSet<u64> = rules.exclude.iter()
            .map(|&id| canonical_related_id(id, &redirects))
            .collect::<Result<_, _>>()?;
        let mut adjacency: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        let link_mentions = related.links.len();
        for link in related.links {
            let from = canonical_related_id(link.from, &redirects)?;
            let to = canonical_related_id(link.to, &redirects)?;
            adjacency.entry(from).or_default().push(to);
        }
        for neighbors in adjacency.values_mut() {
            neighbors.sort_unstable();
        }
        let expanded = expand_related_links(
            &preview.selection.page_ids, &adjacency, &excluded,
            related.max_hops, related.max_new_pages, related.max_edge_visits,
        ).map_err(|error| format!("related expansion rejected: {error:?}"))?;
        if expanded.len() > rules.page_limit {
            return Err("related expansion exceeds topic page limit".into());
        }
        let canonical = serde_json::to_vec(&(
            "foundation-wikipedia/related-selection/v1",
            &preview.selection.snapshot_selection_sha256,
            related.max_hops, related.max_new_pages, related.max_edge_visits,
            &adjacency, &expanded,
        )).map_err(|error| format!("cannot fingerprint related selection: {error}"))?;
        Some(RelatedSelection {
            added_count: expanded.len() - preview.selection.page_ids.len(),
            page_ids: expanded,
            selection_sha256: format!("{:x}", Sha256::digest(&canonical)),
            link_mentions,
        })
    } else {
        None
    };
    let selected_ids = related_report.as_ref()
        .map_or(&preview.selection.page_ids, |report| &report.page_ids);
    let selected_count = selected_ids.len();
    let selection_sha256 = related_report.as_ref().map_or(
        preview.selection.snapshot_selection_sha256.as_str(),
        |report| report.selection_sha256.as_str(),
    );
    if json_output {
        // Machine-readable pre-download preview for a future GUI/builder
        // integration. No field here confers publisher trust or installability.
        let mut result = serde_json::json!({
            "schema_version": 1,
            "source": "local-untrusted-category-graph",
            "input_sha256": input_sha256,
            "source_authenticated": false,
            "publisher_signature_verified": false,
            "offline_ready": false,
            "pack_installed": false,
            "snapshot_label": preview.selection.snapshot_id,

            "definition_sha256": preview.selection.definition_sha256,
            "graph_sha256": preview.selection.graph_sha256,
            "selection_sha256": selection_sha256,

            "selected_page_ids": selected_ids,
            "selected_page_count": selected_count,
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
        if let Some(report) = &related_report {
            let object = result.as_object_mut().expect("preview JSON object");
            object.insert("category_selection_sha256".into(), serde_json::json!(preview.selection.snapshot_selection_sha256));
            object.insert("related_added_page_count".into(), serde_json::json!(report.added_count));
            object.insert("related_link_mentions".into(), serde_json::json!(report.link_mentions));
            object.insert("related_source_authenticated".into(), serde_json::json!(false));
        }
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
    println!("Selected page IDs: {selected_count}");
    if let Some(report) = &related_report {
        println!("Optional related pages added: {}", report.added_count);
        println!("Related link mentions: {}", report.link_mentions);
        println!("Related links source: UNTRUSTED");
        println!("Category selection SHA-256: {}", preview.selection.snapshot_selection_sha256);
    }
    println!("Visited categories: {}", preview.visited_category_count);
    println!(
        "Skipped administrative categories: {}",
        preview.skipped_administrative_count
    );
    println!(
        "Raw article/category mentions inspected: {}/{}",
        preview.inspected_article_mentions, preview.inspected_child_category_mentions
    );
    println!("Input SHA-256 (untrusted local JSON): {input_sha256}");
    println!(
        "Definition SHA-256: {}",
        preview.selection.definition_sha256
    );
    println!("Graph SHA-256: {}", preview.selection.graph_sha256);
    println!(
        "Selection SHA-256: {}",
        selection_sha256
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

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_parent_directory() {
        let root = test_path();
        let link = test_path();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("input.json"), b"{}").unwrap();
        std::os::unix::fs::symlink(&root, &link).unwrap();
        let error = run(&link.join("input.json"), true).unwrap_err();
        fs::remove_file(&link).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(error.contains("contains a symlink"), "{error}");
    }
}
