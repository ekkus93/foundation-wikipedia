//! Byte-level preflight for discovered rendered resource URL candidates.
//!
//! The caller's URL-to-object bindings are untrusted claims. This function
//! checks their completeness and rehashes local object bytes, but cannot prove
//! Wikimedia provenance, source-to-object semantic identity, license accuracy,
//! CSS/HTML safety or an offline-ready pack.

use std::collections::BTreeMap;

use wiki_store::media_inventory::{verify_declared_media, MediaInventoryError, RequiredMedia};
use wiki_store::media_objects::MediaObjectStore;

use crate::rendered_dependencies::RenderDependencyGraph;

#[derive(Debug)]
pub enum RenderedObjectError {
    UnresolvedReferences,
    MissingBinding(String),
    UnexpectedBinding(String),
    ConflictingDigest(String),
    CssBytesMismatch(String),
    InvalidObjects(MediaInventoryError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedObjectAudit {
    pub resource_urls: usize,
    pub verified_objects: usize,
    pub verified_bytes: u64,
    /// This checks only object integrity, not URL-to-byte provenance.
    pub local_object_bytes_verified: bool,
    /// Always false: external source and mapping claims are unauthenticated.
    pub source_authenticated: bool,
    /// Always false until rendering, licensing and attribution are verified.
    pub offline_ready: bool,
}

/// Require an exact URL binding for every discovered HTML/CSS resource,
/// deduplicate identical object digests, and rehash all declared local bytes.
///
/// The graph must have no unresolved dynamic surfaces, unsupported schemes or
/// missing source attributes. Any missing or extra URL binding fails closed.
/// No file is written, and no network request is made.
pub fn audit_rendered_objects(
    graph: &RenderDependencyGraph,
    bindings: &BTreeMap<String, RequiredMedia>,
    store: &MediaObjectStore,
) -> Result<RenderedObjectAudit, RenderedObjectError> {
    if !graph.all_referenced_css_supplied
        || graph.unresolved_dynamic_surfaces != 0
        || graph.unsupported_schemes != 0
        || graph.missing_sources != 0
    {
        return Err(RenderedObjectError::UnresolvedReferences);
    }
    let mut urls = std::collections::BTreeSet::new();
    for candidate in &graph.candidates {
        urls.insert(candidate.url.as_str());
    }
    for url in &urls {
        if !bindings.contains_key(*url) {
            return Err(RenderedObjectError::MissingBinding((*url).to_owned()));
        }
    }
    for url in bindings.keys() {
        if !urls.contains(url.as_str()) {
            return Err(RenderedObjectError::UnexpectedBinding(url.clone()));
        }
    }
    // A CSS inventory produced from one set of source bytes cannot be
    // satisfied by a different local stylesheet with the same URL.
    for (url, (digest, bytes)) in &graph.supplied_css_receipts {
        let bound = bindings
            .get(url)
            .ok_or_else(|| RenderedObjectError::MissingBinding(url.clone()))?;
        if &bound.digest != digest || bound.bytes != *bytes {
            return Err(RenderedObjectError::CssBytesMismatch(url.clone()));
        }
    }
    let mut unique: BTreeMap<String, u64> = BTreeMap::new();
    for entry in bindings.values() {
        match unique.insert(entry.digest.clone(), entry.bytes) {
            Some(previous) if previous != entry.bytes => {
                return Err(RenderedObjectError::ConflictingDigest(entry.digest.clone()));
            }
            _ => {}
        }
    }
    let required: Vec<RequiredMedia> = unique
        .into_iter()
        .map(|(digest, bytes)| RequiredMedia { digest, bytes })
        .collect();
    let verified_bytes =
        verify_declared_media(store, &required).map_err(RenderedObjectError::InvalidObjects)?;
    Ok(RenderedObjectAudit {
        resource_urls: urls.len(),
        verified_objects: required.len(),
        verified_bytes,
        local_object_bytes_verified: true,
        source_authenticated: false,
        offline_ready: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::rendered_dependencies::inventory_rendered_dependencies;

    const DOC: &str = "https://en.wikipedia.org/wiki/Physics";
    const IMAGE: &str = "https://en.wikipedia.org/image.png";

    fn fixture() -> (std::path::PathBuf, MediaObjectStore, RequiredMedia) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-rendered-audit-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        let digest = store.store_bytes(b"diagram").unwrap();
        (root, store, RequiredMedia { digest, bytes: 7 })
    }

    fn graph(markup: &str) -> RenderDependencyGraph {
        inventory_rendered_dependencies(markup, DOC, &BTreeMap::new(), 100).unwrap()
    }

    #[test]
    fn binds_urls_and_verifies_deduplicated_local_bytes() {
        let (root, store, image) = fixture();
        let graph = graph("<img src='/image.png'><img src='/image.png'>");
        let bindings = BTreeMap::from([(IMAGE.to_owned(), image)]);
        let audit = audit_rendered_objects(&graph, &bindings, &store).unwrap();
        assert_eq!(audit.resource_urls, 1);
        assert_eq!(audit.verified_objects, 1);
        assert_eq!(audit.verified_bytes, 7);
        assert!(audit.local_object_bytes_verified);
        assert!(!audit.source_authenticated);
        assert!(!audit.offline_ready);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_extra_or_wrong_size_binding_fails_closed() {
        let (root, store, image) = fixture();
        let graph = graph("<img src='/image.png'>");
        assert!(matches!(
            audit_rendered_objects(&graph, &BTreeMap::new(), &store),
            Err(RenderedObjectError::MissingBinding(_))
        ));
        let mut bindings = BTreeMap::from([(IMAGE.to_owned(), image.clone())]);
        bindings.insert("https://en.wikipedia.org/extra.png".into(), image.clone());
        assert!(matches!(
            audit_rendered_objects(&graph, &bindings, &store),
            Err(RenderedObjectError::UnexpectedBinding(_))
        ));
        bindings.remove("https://en.wikipedia.org/extra.png");
        bindings.get_mut(IMAGE).unwrap().bytes = 8;
        assert!(matches!(
            audit_rendered_objects(&graph, &bindings, &store),
            Err(RenderedObjectError::InvalidObjects(
                MediaInventoryError::WrongSize
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn css_discovery_bytes_must_match_verified_local_stylesheet() {
        let (root, store, image) = fixture();
        let css_text = ".x{background:url(/image.png)}";
        let css_digest = store.store_bytes(css_text.as_bytes()).unwrap();
        let style_url = "https://en.wikipedia.org/style.css";
        let css = BTreeMap::from([(style_url.to_owned(), css_text.to_owned())]);
        let graph = inventory_rendered_dependencies(
            "<link rel='stylesheet' href='/style.css'><img src='/image.png'>",
            DOC,
            &css,
            10,
        )
        .unwrap();
        let mut bindings = BTreeMap::from([
            (IMAGE.to_owned(), image.clone()),
            (
                style_url.to_owned(),
                RequiredMedia {
                    digest: css_digest,
                    bytes: css_text.len() as u64,
                },
            ),
        ]);
        let audit = audit_rendered_objects(&graph, &bindings, &store).unwrap();
        assert_eq!(audit.verified_objects, 2);
        bindings.get_mut(style_url).unwrap().digest = image.digest;
        assert!(matches!(
            audit_rendered_objects(&graph, &bindings, &store),
            Err(RenderedObjectError::CssBytesMismatch(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_bytes_and_unresolved_dynamic_surfaces_are_rejected() {
        let (root, store, image) = fixture();
        let bindings = BTreeMap::from([(IMAGE.to_owned(), image.clone())]);
        let dynamic = graph("<img src='/image.png'><canvas></canvas>");
        assert!(matches!(
            audit_rendered_objects(&dynamic, &bindings, &store),
            Err(RenderedObjectError::UnresolvedReferences)
        ));
        let graph = graph("<img src='/image.png'>");
        fs::write(
            root.join(&image.digest[..2]).join(&image.digest),
            b"tampered",
        )
        .unwrap();
        assert!(matches!(
            audit_rendered_objects(&graph, &bindings, &store),
            Err(RenderedObjectError::InvalidObjects(
                MediaInventoryError::Unreadable(_)
            ))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
