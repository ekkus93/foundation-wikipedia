//! Local category-graph preview, not a publisher-verified source importer.
//! The JSON input is explicitly untrusted; no pack is built or installed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
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

pub fn run(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|error| format!("cannot open preview input: {error}"))?;
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
            || category.children.iter().any(|name| !safe_category_name(name))
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
    println!("Local topic preview; source publication/authenticity: NOT VERIFIED");
    println!("No media fetched; no pack built, signed, or installed");
    println!("Snapshot label (untrusted): {}", preview.selection.snapshot_id);
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
    println!("Definition SHA-256: {}", preview.selection.definition_sha256);
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
