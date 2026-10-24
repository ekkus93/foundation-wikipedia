//! Offline discovery of the editable curated topic seeds.
//! These are bounded definitions, not Wikimedia-authenticated snapshots or packs.

use wiki_pack_builder::curated::INITIAL_TOPICS;

fn catalog(json_output: bool) -> Result<String, String> {
    if json_output {
        let topics: Vec<_> = INITIAL_TOPICS
            .iter()
            .map(|topic| {
                serde_json::json!({
                    "id": topic.id,
                    "label": topic.label,
                    "project": topic.project,
                    "roots": topic.roots,
                    "depth_limit": topic.depth_limit,
                    "page_limit": topic.page_limit,
                })
            })
            .collect();
        let response = serde_json::json!({
            "schema_version": 1,
            "kind": "curated-topic-seeds",
            "source_authenticated": false,
            "publisher_signature_verified": false,
            "offline_ready": false,
            "pack_installed": false,
            "topics": topics,
        });
        return serde_json::to_string(&response)
            .map_err(|error| format!("cannot serialize curated topic catalog: {error}"));
    }
    let mut lines = vec![
        "Curated topic seeds (editable definitions; NOT authenticated packs)".to_owned(),
        "No source fetched; no pack built, signed, or installed.".to_owned(),
    ];
    for topic in INITIAL_TOPICS {
        lines.push(format!(
            "{} — {} [{}], depth <= {}, pages <= {}",
            topic.id, topic.label, topic.project, topic.depth_limit, topic.page_limit
        ));
        for root in topic.roots {
            lines.push(format!("  root: {root}"));
        }
    }
    lines.push("Use these IDs in preview-topic JSON curated_topics; verify real source coverage independently.".to_owned());
    Ok(lines.join("\n"))
}

pub fn run(json_output: bool) -> Result<(), String> {
    println!("{}", catalog(json_output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_catalog_matches_editable_definitions_and_denies_trust_claims() {
        let first = catalog(true).unwrap();
        assert_eq!(first, catalog(true).unwrap());
        let value: serde_json::Value = serde_json::from_str(&first).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["source_authenticated"], false);
        assert_eq!(value["publisher_signature_verified"], false);
        assert_eq!(value["offline_ready"], false);
        assert_eq!(value["pack_installed"], false);
        let topics = value["topics"].as_array().unwrap();
        assert_eq!(topics.len(), INITIAL_TOPICS.len());
        for (actual, expected) in topics.iter().zip(INITIAL_TOPICS) {
            assert_eq!(actual["id"], expected.id);
            assert_eq!(actual["label"], expected.label);
            assert_eq!(actual["project"], expected.project);
            assert_eq!(actual["depth_limit"], expected.depth_limit);
            assert_eq!(actual["page_limit"], expected.page_limit);
            assert_eq!(
                actual["roots"],
                serde_json::to_value(expected.roots).unwrap()
            );
        }
    }

    #[test]
    fn human_catalog_names_all_topics_and_disclaims_publication_trust() {
        let display = catalog(false).unwrap();
        assert!(display.contains("NOT authenticated packs"));
        assert!(display.contains("no pack built, signed, or installed"));
        for topic in INITIAL_TOPICS {
            assert!(display.contains(topic.id));
            assert!(display.contains(topic.roots[0]));
        }
    }
}
