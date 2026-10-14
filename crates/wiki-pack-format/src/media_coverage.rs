//! Preflight for typed mandatory article-media references against a pack manifest.
//!
//! This is a necessary but NOT sufficient offline-completeness check: it does
//! not inspect rendered HTML for undiscovered resources, validate MIME from
//! bytes, verify the object's actual hash, or authenticate publisher origin.
//! Run object-byte verification and rendered-resource discovery separately.

use std::collections::{BTreeMap, BTreeSet};

use wiki_model::{Article, ModelError};

use crate::manifest::{Manifest, ManifestError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaCoverageError {
    InvalidArticle(ModelError),
    InvalidManifest(ManifestError),
    MissingDigest(usize),
    MissingMediaObject(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaCoverage {
    /// Number of typed media references, including required A/V previews.
    pub references: usize,
    /// Number of distinct referenced media content digests.
    pub unique_objects: usize,
    /// Sum of declared bytes for those distinct media objects.
    pub declared_bytes: u64,
}

/// Require every typed media reference to have a content digest and a
/// manifest-declared media object with that digest. A matching article/index
/// object does not qualify as a media resource. This is manifest preflight,
/// not proof of installed or render-complete offline visuals.
pub fn audit_article_media(
    article: &Article,
    manifest: &Manifest,
) -> Result<MediaCoverage, MediaCoverageError> {
    article
        .validate()
        .map_err(MediaCoverageError::InvalidArticle)?;
    manifest
        .validate()
        .map_err(MediaCoverageError::InvalidManifest)?;
    if article.key.project != manifest.project {
        return Err(MediaCoverageError::InvalidArticle(
                ModelError::InvalidIdentity
            ));
    }

    let mut media = BTreeMap::new();
    for object in &manifest.objects {
        if object.path.starts_with("media/") {
            media
                .entry(object.sha256.to_ascii_lowercase())
                .or_insert(object.bytes);
        }
    }
    let mut unique = BTreeSet::new();
    let mut bytes = 0u64;
    for (index, asset) in article.media.iter().enumerate() {
        let digest = asset
            .sha256
            .as_ref()
            .ok_or(MediaCoverageError::MissingDigest(index))?
            .to_ascii_lowercase();
        let object_bytes = *media
            .get(&digest)
            .ok_or(MediaCoverageError::MissingMediaObject(index))?;
        if unique.insert(digest) {
            // The manifest already bounds the total declared object bytes.
            bytes += object_bytes;
        }
    }
    Ok(MediaCoverage {
        references: article.media.len(),
        unique_objects: unique.len(),
        declared_bytes: bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Object, Origin, FORMAT_VERSION};
    use wiki_model::{ArticleKey, MediaAsset, Revision, ARTICLE_SCHEMA_VERSION};

    fn article() -> Article {
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 7,
                timestamp: "2026-10-14T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Physics".into(),
            display_title: "Physics".into(),
            language: "en".into(),
            namespace: 0,
            aliases: vec![],
            wikidata_id: None,
            lead: vec![],
            sections: vec![],
            references: vec![],
            links: vec![],
            media: vec![MediaAsset {
                source_url: "https://upload.wikimedia.org/figure.svg".into(),
                mime_type: "image/svg+xml".into(),
                sha256: Some("b".repeat(64)),
                license: "CC BY-SA".into(),
                creator: "Contributor".into(),
                attribution: "Contributor under CC BY-SA".into(),
                is_av_preview: false,
            }],
            rendered_html: "<p>Physics</p>".into(),
            is_disambiguation: false,
        }
    }

    fn manifest() -> Manifest {
        Manifest {
            version: FORMAT_VERSION,
            pack_id: "physics".into(),
            project: "enwiki".into(),
            snapshot: "20261014".into(),
            origin: Origin::Custom {
                definition_id: "physics".into(),
            },
            objects: vec![Object {
                path: "media/figure.svg".into(),
                sha256: "b".repeat(64),
                bytes: 1024,
            }],
        }
    }

    #[test]
    fn accepts_typed_media_and_deduplicates_shared_av_preview() {
        let mut a = article();
        a.media.push(MediaAsset {
            is_av_preview: true,
            ..a.media[0].clone()
        });
        let report = audit_article_media(&a, &manifest()).unwrap();
        assert_eq!(report.references, 2);
        assert_eq!(report.unique_objects, 1);
        assert_eq!(report.declared_bytes, 1024);
    }

    #[test]
    fn fails_closed_on_missing_digest_or_missing_media_object() {
        let mut a = article();
        a.media[0].sha256 = None;
        assert_eq!(
            audit_article_media(&a, &manifest()),
            Err(MediaCoverageError::MissingDigest(0))
        );
        a.media[0].sha256 = Some("c".repeat(64));
        assert_eq!(
            audit_article_media(&a, &manifest()),
            Err(MediaCoverageError::MissingMediaObject(0))
        );

        let mut pack = manifest();
        pack.objects[0].path = "articles/42.cbor".into();
        assert_eq!(
            audit_article_media(&article(), &pack),
            Err(MediaCoverageError::MissingMediaObject(0))
        );
    }

    #[test]
    fn rejects_cross_project_and_invalid_manifest_without_trust_claim() {
        let mut pack = manifest();
        pack.project = "frwiki".into();
        assert_eq!(
            audit_article_media(&article(), &pack),
            Err(MediaCoverageError::InvalidArticle(ModelError::InvalidIdentity))
        );
        pack = manifest();
        pack.objects[0].bytes = 0;
        assert!(matches!(
            audit_article_media(&article(), &pack),
            Err(MediaCoverageError::InvalidManifest(_))
        ));
    }
}
