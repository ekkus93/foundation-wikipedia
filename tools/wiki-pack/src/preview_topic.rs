//! Local category-graph preview, not a publisher-verified source importer.
//! The JSON input is explicitly untrusted; no pack is built or installed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use wiki_pack_builder::curated::curated_topic;
use wiki_pack_builder::curated_preview::curated_definition_fingerprint;
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
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    curated_topics: Vec<String>,
    #[serde(default)]
    include: Vec<u64>,
    #[serde(default)]
    exclude: Vec<u64>,
    #[serde(default)]
    depth_limit: Option<usize>,
    #[serde(default)]
    page_limit: Option<usize>,
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

/// Wikimedia title keys treat underscores and spaces identically. Use one
/// spelling for roots, category nodes and child references before hashing
/// or resolving the local JSON graph. Reject malformed normalized names.
fn canonical_category_name(name: &str) -> Result<String, String> {
    if !safe_category_name(name) {
        return Err("invalid category name".into());
    }
    let canonical = name.replace('_', " ");
    let body = canonical.strip_prefix("Category:").unwrap_or(&canonical);
    if !safe_category_name(&canonical) || body.is_empty() || body.trim() != body {
        return Err("invalid category name".into());
    }
    Ok(canonical)
}

/// Expand named curated topic seeds into the local graph preview rules.
/// The snapshot label is only a caller claim; this checks edition consistency,
/// not official publication authenticity or category coverage. Legacy previews
/// with no curated IDs preserve their existing root and limit semantics.
fn apply_curated_seeds(input: &mut PreviewInput) -> Result<Vec<String>, String> {
    if input.curated_topics.len() > 32 {
        return Err("too many curated topic IDs".into());
    }
    let mut ids = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut policy = None;
    for id in &input.curated_topics {
        let topic = curated_topic(id).ok_or_else(|| format!("unknown curated topic: {id}"))?;
        // A prefix-only match accepts an empty or ambiguous generation
        // such as `enwiki-`. Require a concrete local generation label.
        let generation = input
            .snapshot_id
            .strip_prefix(&format!("{}-", topic.project));
        if !generation.is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix.len() <= 255
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
                && suffix.bytes().any(|byte| byte.is_ascii_alphanumeric())
        }) {
            return Err(format!(
                "curated topic {id} requires {} snapshot label",
                topic.project
            ));
        }
        if policy
            .is_some_and(|(depth, pages)| depth != topic.depth_limit || pages != topic.page_limit)
        {
            return Err("curated topics have incompatible safety policies".into());
        }
        policy = Some((topic.depth_limit, topic.page_limit));
        if input.depth_limit.unwrap_or(topic.depth_limit) > topic.depth_limit
            || input.page_limit.unwrap_or(topic.page_limit) > topic.page_limit
        {
            return Err(format!("curated topic {id} exceeds seed safety limits"));
        }
        ids.insert(id.clone());
        for root in topic.roots {
            roots.insert((*root).to_owned());
        }
    }
    if let Some((depth, pages)) = policy {
        // A curated-only request can use the published seed's conservative
        // defaults; explicit tighter limits remain user-controlled.
        input.depth_limit.get_or_insert(depth);
        input.page_limit.get_or_insert(pages);
        roots.extend(input.roots.iter().cloned());
        input.roots = roots.into_iter().collect();
    } else if input.depth_limit.is_none() || input.page_limit.is_none() {
        return Err("custom topic previews require explicit depth_limit and page_limit".into());
    }
    Ok(ids.into_iter().collect())
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
    let mut input: PreviewInput =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid preview JSON: {error}"))?;
    let curated_ids = apply_curated_seeds(&mut input)?;
    if input.categories.len() > MAX_CATEGORIES
        || input.redirects.len() > MAX_EDGES
        || input.disambiguations.len() > MAX_EDGES
        || input.roots.len() > MAX_CATEGORIES
    {
        return Err("preview source exceeds category or redirect limits".into());
    }
    // Normalize user-supplied roots just like category pages and edges.
    // Otherwise equivalent MediaWiki spellings can silently miss a root.
    let roots = input
        .roots
        .iter()
        .map(|name| canonical_category_name(name))
        .collect::<Result<Vec<_>, _>>()?;
    let mut graph = BTreeMap::new();
    let mut edges = 0usize;
    for category in input.categories {
        let name = canonical_category_name(&category.name)?;
        let children = category
            .children
            .iter()
            .map(|child| canonical_category_name(child))
            .collect::<Result<Vec<_>, _>>()?;
        if category.articles.contains(&0) {
            return Err("invalid category member or name".into());
        }
        edges = edges
            .checked_add(category.articles.len())
            .and_then(|count| count.checked_add(children.len()))
            .filter(|count| *count <= MAX_EDGES)
            .ok_or_else(|| "preview category graph exceeds member limit".to_owned())?;
        if graph
            .insert(
                name,
                Category {
                    articles: category.articles,
                    children,
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
        roots,
        include: input.include,
        exclude: input.exclude,
        depth_limit: input
            .depth_limit
            .ok_or_else(|| "missing preview depth_limit".to_owned())?,
        page_limit: input
            .page_limit
            .ok_or_else(|| "missing preview page_limit".to_owned())?,
    };
    let preview = preview_pinned_selection(
        &graph,
        &rules,
        &redirects,
        &input.disambiguations.into_iter().collect::<BTreeSet<_>>(),
        &input.snapshot_id,
    )
    .map_err(|error| format!("topic preview rejected: {error:?}"))?;
    // Bind curated seed identity to the effective roots/overrides in the
    // same definition receipt used by the revision-joined library preview.
    let curated_fingerprint = if curated_ids.is_empty() {
        None
    } else {
        let ids: Vec<&str> = curated_ids.iter().map(String::as_str).collect();
        Some(
            curated_definition_fingerprint(&ids, &preview.selection.definition_sha256)
                .map_err(|error| format!("curated definition fingerprint rejected: {error:?}"))?,
        )
    };
    let related_report = if let Some(related) = related {
        if related.links.len() > MAX_EDGES {
            return Err("related links exceed the member limit".into());
        }
        let excluded: BTreeSet<u64> = rules
            .exclude
            .iter()
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
            &preview.selection.page_ids,
            &adjacency,
            &excluded,
            related.max_hops,
            related.max_new_pages,
            related.max_edge_visits,
        )
        .map_err(|error| format!("related expansion rejected: {error:?}"))?;
        if expanded.len() > rules.page_limit {
            return Err("related expansion exceeds topic page limit".into());
        }
        let canonical = serde_json::to_vec(&(
            "foundation-wikipedia/related-selection/v1",
            &preview.selection.snapshot_selection_sha256,
            related.max_hops,
            related.max_new_pages,
            related.max_edge_visits,
            &adjacency,
            &expanded,
        ))
        .map_err(|error| format!("cannot fingerprint related selection: {error}"))?;
        Some(RelatedSelection {
            added_count: expanded.len() - preview.selection.page_ids.len(),
            page_ids: expanded,
            selection_sha256: format!("{:x}", Sha256::digest(&canonical)),
            link_mentions,
        })
    } else {
        None
    };
    let selected_ids = related_report
        .as_ref()
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
            "depth_limit": rules.depth_limit,
            "page_limit": rules.page_limit,
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
        if let Some(digest) = &curated_fingerprint {
            let object = result.as_object_mut().expect("preview JSON object");
            object.insert("curated_topic_ids".into(), serde_json::json!(curated_ids));
            object.insert(
                "curated_definition_sha256".into(),
                serde_json::json!(digest),
            );
        }
        if let Some(report) = &related_report {
            let object = result.as_object_mut().expect("preview JSON object");
            object.insert(
                "category_selection_sha256".into(),
                serde_json::json!(preview.selection.snapshot_selection_sha256),
            );
            object.insert(
                "related_added_page_count".into(),
                serde_json::json!(report.added_count),
            );
            object.insert(
                "related_link_mentions".into(),
                serde_json::json!(report.link_mentions),
            );
            object.insert(
                "related_source_authenticated".into(),
                serde_json::json!(false),
            );
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
    if let Some(digest) = &curated_fingerprint {
        println!("Curated topic seeds: {}", curated_ids.join(", "));
        println!("Curated definition SHA-256: {digest}");
    }
    println!("Selected page IDs: {selected_count}");
    println!("Topic depth limit: {}", rules.depth_limit);
    println!("Topic page limit: {}", rules.page_limit);
    if let Some(report) = &related_report {
        println!("Optional related pages added: {}", report.added_count);
        println!("Related link mentions: {}", report.link_mentions);
        println!("Related links source: UNTRUSTED");
        println!(
            "Category selection SHA-256: {}",
            preview.selection.snapshot_selection_sha256
        );
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
    println!("Selection SHA-256: {}", selection_sha256);
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

    #[test]
    fn curated_seeds_expand_deterministically_and_enforce_edition_and_limits() {
        let mut input = PreviewInput {
            snapshot_id: "enwiki-20261023".into(),
            roots: vec!["Category:Extra".into()],
            curated_topics: vec!["physics".into(), "mathematics".into(), "physics".into()],
            include: vec![],
            exclude: vec![],
            depth_limit: Some(2),
            page_limit: Some(50_000),
            categories: vec![],
            redirects: vec![],
            disambiguations: vec![],
            related: None,
        };
        assert_eq!(
            apply_curated_seeds(&mut input).unwrap(),
            vec!["mathematics", "physics"]
        );
        assert_eq!(
            input.roots,
            vec!["Category:Extra", "Category:Mathematics", "Category:Physics"]
        );
        let ids = ["mathematics", "physics"];
        let digest = "a".repeat(64);
        assert_eq!(
            curated_definition_fingerprint(&ids, &digest),
            curated_definition_fingerprint(&["physics", "mathematics", "physics"], &digest)
        );
        let mut wrong_edition = PreviewInput {
            snapshot_id: "frwiki-20261023".into(),
            curated_topics: vec!["physics".into()],
            ..input
        };
        assert!(apply_curated_seeds(&mut wrong_edition)
            .unwrap_err()
            .contains("enwiki"));
        wrong_edition.snapshot_id = "enwiki-20261023".into();
        wrong_edition.page_limit = Some(50_001);
        assert!(apply_curated_seeds(&mut wrong_edition)
            .unwrap_err()
            .contains("safety limits"));
        wrong_edition.page_limit = Some(50_000);
        wrong_edition.depth_limit = Some(3);
        assert!(apply_curated_seeds(&mut wrong_edition)
            .unwrap_err()
            .contains("safety limits"));
        wrong_edition.depth_limit = Some(2);
        wrong_edition.curated_topics = vec!["not-a-topic".into()];
        assert!(apply_curated_seeds(&mut wrong_edition)
            .unwrap_err()
            .contains("unknown curated topic"));
        wrong_edition.curated_topics = vec!["physics".into(); 33];
        assert!(apply_curated_seeds(&mut wrong_edition)
            .unwrap_err()
            .contains("too many"));
        wrong_edition.curated_topics = vec!["physics".into()];
        for invalid in [
            "enwiki-",
            "enwiki-...",
            "enwiki-2026/10/23",
            "enwiki-2026\n",
        ] {
            wrong_edition.snapshot_id = invalid.into();
            assert!(
                apply_curated_seeds(&mut wrong_edition).is_err(),
                "{invalid}"
            );
        }
        wrong_edition.snapshot_id = "enwiki-20261023".into();
        assert!(apply_curated_seeds(&mut wrong_edition).is_ok());

        let mut implicit = PreviewInput {
            depth_limit: None,
            page_limit: None,
            curated_topics: vec!["mathematics".into()],
            ..wrong_edition
        };
        assert_eq!(
            apply_curated_seeds(&mut implicit).unwrap(),
            vec!["mathematics"]
        );
        assert_eq!(implicit.depth_limit, Some(2));
        assert_eq!(implicit.page_limit, Some(50_000));
        implicit.curated_topics.clear();
        implicit.depth_limit = None;
        assert!(apply_curated_seeds(&mut implicit)
            .unwrap_err()
            .contains("require explicit"));
    }

    #[test]
    fn json_preview_normalizes_category_underscores_and_rejects_alias_nodes() {
        assert_eq!(
            canonical_category_name("Category:Quantum_mechanics").unwrap(),
            "Category:Quantum mechanics"
        );
        for invalid in ["Category:_Quantum", "Category:Quantum_", "_", "Category:"] {
            assert!(canonical_category_name(invalid).is_err(), "{invalid}");
        }

        let path = test_path();
        let valid = serde_json::json!({
            "snapshot_id": "enwiki-20261023",
            "roots": ["Category:Quantum mechanics"],
            "depth_limit": 1,
            "page_limit": 100,
            "categories": [
                {"name": "Category:Quantum_mechanics", "articles": [42],
                 "children": ["Category:Wave_mechanics"]},
                {"name": "Category:Wave mechanics", "articles": [43]}
            ]
        });
        fs::write(&path, valid.to_string()).unwrap();
        assert!(run(&path, true).is_ok());

        // Distinct raw nodes cannot alias the same canonical category.
        let duplicate = serde_json::json!({
            "snapshot_id": "enwiki-20261023",
            "roots": ["Category:Quantum_mechanics"],
            "depth_limit": 1,
            "page_limit": 100,
            "categories": [
                {"name": "Category:Quantum_mechanics", "articles": [42]},
                {"name": "Category:Quantum mechanics", "articles": [43]}
            ]
        });
        fs::write(&path, duplicate.to_string()).unwrap();
        let result = run(&path, true);
        fs::remove_file(&path).unwrap();
        assert!(result.unwrap_err().contains("duplicate category name"));
    }

    #[test]
    fn curated_only_json_preview_runs_without_explicit_roots() {
        let path = test_path();
        let json = serde_json::json!({
            "snapshot_id": "enwiki-20261023",
            "curated_topics": ["mathematics"],
            "categories": [
                {"name": "Category:Mathematics", "articles": [42]}
            ]
        });
        fs::write(&path, json.to_string()).unwrap();
        let result = run(&path, true);
        fs::remove_file(&path).unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
}
