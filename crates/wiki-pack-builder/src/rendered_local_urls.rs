//! Provisional content-addressed local URI plan for verified rendered resources.
//!
//! This is a pure planning step, not HTML/CSS rewriting or a WebView handler.
//! The caller's URL-to-digest bindings remain unauthenticated. A platform
//! handler must authorize each URI against an installed pack's owner inventory
//! and rehash the object on read; this plan alone never certifies offline use.

use std::collections::BTreeMap;

use wiki_store::media_inventory::RequiredMedia;
use wiki_store::media_objects::{MediaObjectError, MediaObjectStore};
use wiki_store::media_ownership::{MediaRegistry, MediaRegistryError, OwnerKind};

use crate::rendered_dependencies::RenderDependencyGraph;
use crate::rendered_objects::{audit_rendered_objects, RenderedObjectAudit, RenderedObjectError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedLocalUrlPlan {
    /// Deterministic original absolute URL -> logical content-addressed URI.
    /// This is not a filesystem path or an authorization token.
    pub source_to_local_uri: BTreeMap<String, String>,
    pub audit: RenderedObjectAudit,
    /// False until an origin-restricted desktop/Android handler is qualified.
    pub handler_qualified: bool,
    /// False until safe HTML/CSS rewriting and rendering are qualified.
    pub rewritten_content_qualified: bool,
}

/// Build a stable logical URI mapping only after byte-level graph preflight.
/// Duplicate source URLs share a mapping; distinct URLs for the same digest
/// share one logical object URI. No network access or file writes occur.
pub fn plan_verified_local_urls(
    graph: &RenderDependencyGraph,
    bindings: &BTreeMap<String, RequiredMedia>,
    store: &MediaObjectStore,
) -> Result<VerifiedLocalUrlPlan, RenderedObjectError> {
    let audit = audit_rendered_objects(graph, bindings, store)?;
    let source_to_local_uri = bindings
        .iter()
        .map(|(url, object)| {
            (
                url.clone(),
                format!("wiki-asset://sha256/{}", object.digest),
            )
        })
        .collect();
    Ok(VerifiedLocalUrlPlan {
        source_to_local_uri,
        audit,
        handler_qualified: false,
        rewritten_content_qualified: false,
    })
}

/// A single owner-scoped, on-demand local-object read. This is only a
/// building block for a future scheme handler; it does not authenticate the
/// caller's URL plan, HTML, pack manifest, media license, or source provenance.
#[derive(Debug)]
pub enum LocalAssetReadError {
    InvalidUri,
    NotInPlan,
    Registry(MediaRegistryError),
    NotOwned,
    Object(MediaObjectError),
    WrongSize,
}

/// Resolve only canonical digest URIs present in this plan, and require a
/// currently registered *pack* owner before rehashing the bytes on every read.
/// A cache or user-pin owner alone cannot authorize this pack's resource.
/// The platform adapter must enforce its own origin, pack and MIME policies.
pub fn read_verified_pack_asset(
    plan: &VerifiedLocalUrlPlan,
    local_uri: &str,
    registry: &MediaRegistry,
    pack_id: &str,
    store: &MediaObjectStore,
) -> Result<Vec<u8>, LocalAssetReadError> {
    let digest = local_uri
        .strip_prefix("wiki-asset://sha256/")
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or(LocalAssetReadError::InvalidUri)?;
    if !plan
        .source_to_local_uri
        .values()
        .any(|uri| uri == local_uri)
    {
        return Err(LocalAssetReadError::NotInPlan);
    }
    let owned = registry
        .owned_media(OwnerKind::Pack, pack_id)
        .map_err(LocalAssetReadError::Registry)?;
    let expected_bytes = owned
        .iter()
        .find(|(owned_digest, _)| owned_digest == digest)
        .map(|(_, bytes)| *bytes)
        .ok_or(LocalAssetReadError::NotOwned)?;
    let bytes = store
        .read_verified(digest)
        .map_err(LocalAssetReadError::Object)?;
    if bytes.len() as u64 != expected_bytes {
        return Err(LocalAssetReadError::WrongSize);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rendered_dependencies::inventory_rendered_dependencies;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    const DOC: &str = "https://en.wikipedia.org/wiki/Physics";

    fn store_fixture() -> (std::path::PathBuf, MediaObjectStore) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "wiki-local-url-plan-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let store = MediaObjectStore::new(&root);
        (root, store)
    }

    #[test]
    fn maps_deduplicated_html_and_css_urls_to_verified_digest_uris() {
        let (root, store) = store_fixture();
        let digest = store.store_bytes(b"image").unwrap();
        let css_text = ".x{background:url(/a.png)}";
        let css_digest = store.store_bytes(css_text.as_bytes()).unwrap();
        let css_url = "https://en.wikipedia.org/style.css";
        let image_a = "https://en.wikipedia.org/a.png";
        let image_b = "https://en.wikipedia.org/b.png";
        let css = BTreeMap::from([(css_url.to_owned(), css_text.to_owned())]);
        let graph = inventory_rendered_dependencies(
            "<link rel='stylesheet' href='/style.css'><img src='/a.png'><img src='/b.png'>",
            DOC,
            &css,
            12,
        )
        .unwrap();
        let bindings = BTreeMap::from([
            (
                image_a.to_owned(),
                RequiredMedia {
                    digest: digest.clone(),
                    bytes: 5,
                },
            ),
            (
                image_b.to_owned(),
                RequiredMedia {
                    digest: digest.clone(),
                    bytes: 5,
                },
            ),
            (
                css_url.to_owned(),
                RequiredMedia {
                    digest: css_digest.clone(),
                    bytes: css_text.len() as u64,
                },
            ),
        ]);
        let plan = plan_verified_local_urls(&graph, &bindings, &store).unwrap();
        assert_eq!(plan.source_to_local_uri.len(), 3);
        assert_eq!(
            plan.source_to_local_uri[image_a],
            format!("wiki-asset://sha256/{digest}")
        );
        assert_eq!(
            plan.source_to_local_uri[image_a],
            plan.source_to_local_uri[image_b]
        );
        assert_eq!(
            plan.source_to_local_uri[css_url],
            format!("wiki-asset://sha256/{css_digest}")
        );
        assert_eq!(plan.audit.verified_objects, 2);
        assert!(!plan.audit.source_authenticated);
        assert!(!plan.audit.offline_ready);
        assert!(!plan.handler_qualified);
        assert!(!plan.rewritten_content_qualified);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn local_reads_require_exact_pack_owner_and_rehash_bytes() {
        use wiki_store::media_ownership::{MediaNotice, MediaRegistry, OwnerKind};

        let (root, store) = store_fixture();
        let digest = store.store_bytes(b"image").unwrap();
        let image_url = "https://en.wikipedia.org/a.png";
        let graph = inventory_rendered_dependencies("<img src='/a.png'>", DOC, &BTreeMap::new(), 8)
            .unwrap();
        let bindings = BTreeMap::from([(
            image_url.to_owned(),
            RequiredMedia {
                digest: digest.clone(),
                bytes: 5,
            },
        )]);
        let plan = plan_verified_local_urls(&graph, &bindings, &store).unwrap();
        let uri = &plan.source_to_local_uri[image_url];
        let mut registry = MediaRegistry::in_memory().unwrap();
        let notice = MediaNotice {
            mime: "image/png".into(),
            source_url: image_url.into(),
            creator: "Test fixture".into(),
            license: "CC BY-SA 4.0".into(),
        };
        assert_eq!(
            registry.register_verified_bytes(b"image", &notice).unwrap(),
            digest
        );
        registry
            .add_owner(&digest, OwnerKind::Cache, "physics")
            .unwrap();
        assert!(matches!(
            read_verified_pack_asset(&plan, uri, &registry, "physics", &store),
            Err(LocalAssetReadError::NotOwned)
        ));
        registry
            .add_owner(&digest, OwnerKind::Pack, "physics")
            .unwrap();
        assert_eq!(
            read_verified_pack_asset(&plan, uri, &registry, "physics", &store).unwrap(),
            b"image"
        );
        assert!(matches!(
            read_verified_pack_asset(&plan, uri, &registry, "other", &store),
            Err(LocalAssetReadError::NotOwned)
        ));
        for bad_uri in [
            "wiki-asset://sha256/",
            "wiki-asset://sha256/../../etc/passwd",
            "wiki-asset://sha256/ABCDEF",
            "wiki-asset://sha256/0?x=1",
        ] {
            assert!(matches!(
                read_verified_pack_asset(&plan, bad_uri, &registry, "physics", &store),
                Err(LocalAssetReadError::InvalidUri)
            ));
        }
        let valid_but_not_planned = format!("wiki-asset://sha256/{}", "0".repeat(64));
        assert!(matches!(
            read_verified_pack_asset(&plan, &valid_but_not_planned, &registry, "physics", &store),
            Err(LocalAssetReadError::NotInPlan)
        ));
        fs::write(root.join(&digest[..2]).join(&digest), b"wrong").unwrap();
        assert!(matches!(
            read_verified_pack_asset(&plan, uri, &registry, "physics", &store),
            Err(LocalAssetReadError::Object(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_unverified_bindings_and_dynamic_surfaces() {
        let (root, store) = store_fixture();
        let digest = store.store_bytes(b"image").unwrap();
        let graph = inventory_rendered_dependencies("<img src='/a.png'>", DOC, &BTreeMap::new(), 8)
            .unwrap();
        let bindings = BTreeMap::from([(
            "https://en.wikipedia.org/a.png".to_owned(),
            RequiredMedia {
                digest: digest.clone(),
                bytes: 5,
            },
        )]);
        assert!(matches!(
            plan_verified_local_urls(&graph, &BTreeMap::new(), &store),
            Err(RenderedObjectError::MissingBinding(_))
        ));
        let dynamic = inventory_rendered_dependencies(
            "<img src='/a.png'><canvas></canvas>",
            DOC,
            &BTreeMap::new(),
            8,
        )
        .unwrap();
        assert!(matches!(
            plan_verified_local_urls(&dynamic, &bindings, &store),
            Err(RenderedObjectError::UnresolvedReferences)
        ));
        fs::write(root.join(&digest[..2]).join(&digest), b"tampered").unwrap();
        assert!(matches!(
            plan_verified_local_urls(&graph, &bindings, &store),
            Err(RenderedObjectError::InvalidObjects(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
