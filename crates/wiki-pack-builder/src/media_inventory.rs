//! Discover image references from typed canonical article blocks.
//!
//! This is intentionally NOT an offline-completeness certificate. The final
//! renderer must separately scan sanitized HTML, CSS, fonts, MathML and
//! infobox output. Opaque HTML fallback is rejected until that scan exists.

use std::collections::{BTreeMap, BTreeSet};
use wiki_store::media_inventory::RequiredMedia;
use wiki_store::media_objects::MediaObjectStore;
use wiki_model::{Article, Block, BlockContent, ModelError, Section};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryError {
    InvalidArticle(ModelError),
    UnresolvedHtmlFallback,
    MissingDigest(usize),
    UnverifiedObject(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredMediaInventory {
    /// Indices into Article.media, deterministic and deduplicated.
    pub referenced_media: Vec<usize>,
    /// Must remain false until rendered HTML/CSS dependencies are inspected.
    pub rendered_dependencies_verified: bool,
}

/// Revision-scoped offline attribution for a referenced structured media item.
/// These fields are untrusted text; any UI must escape/sanitize on display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredMediaNotice {
    pub project: String,
    pub page_id: u64,
    pub revision_id: u64,
    pub media_index: usize,
    pub source_url: String,
    pub mime_type: String,
    pub license: String,
    pub creator: String,
    pub attribution: String,
    pub is_av_preview: bool,
}

/// Produce deterministic, de-duplicated attribution metadata for references
/// found in typed blocks. This does NOT establish offline completeness.
pub fn collect_structured_media_notices(
    article: &Article,
) -> Result<Vec<StructuredMediaNotice>, InventoryError> {
    let inventory = collect_structured_media(article)?;
    Ok(inventory
        .referenced_media
        .into_iter()
        .map(|index| {
            let media = &article.media[index];
            StructuredMediaNotice {
                project: article.key.project.clone(),
                page_id: article.key.page_id,
                revision_id: article.revision.revision_id,
                media_index: index,
                source_url: media.source_url.clone(),
                mime_type: media.mime_type.clone(),
                license: media.license.clone(),
                creator: media.creator.clone(),
                attribution: media.attribution.clone(),
                is_av_preview: media.is_av_preview,
            }
        })
        .collect())
}

pub fn collect_structured_media(
    article: &Article,
) -> Result<StructuredMediaInventory, InventoryError> {
    article.validate().map_err(InventoryError::InvalidArticle)?;
    let mut media = BTreeSet::new();

    fn walk(blocks: &[Block], media: &mut BTreeSet<usize>) -> Result<(), InventoryError> {
        for block in blocks {
            match &block.content {
                BlockContent::Media { media_index } => {
                    media.insert(*media_index);
                }
                BlockContent::HtmlFallback(_) => {
                    return Err(InventoryError::UnresolvedHtmlFallback);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn walk_sections(
        sections: &[Section],
        media: &mut BTreeSet<usize>,
    ) -> Result<(), InventoryError> {
        for section in sections {
            walk(&section.blocks, media)?;
            walk_sections(&section.subsections, media)?;
        }
        Ok(())
    }

    walk(&article.lead, &mut media)?;
    walk_sections(&article.sections, &mut media)?;
    Ok(StructuredMediaInventory {
        referenced_media: media.into_iter().collect(),
        rendered_dependencies_verified: false,
    })
}

/// Rehash every structured media object referenced by the exact canonical
/// article revision. Multiple references to identical bytes are deduplicated
/// deterministically; attribution is retained separately by
/// collect_structured_media_notices.
///
/// This does NOT certify offline completeness: rendered HTML, CSS, fonts,
/// equations and other renderer dependencies still require a separate scan.
pub fn verify_structured_media_objects(
    article: &Article,
    store: &MediaObjectStore,
) -> Result<Vec<RequiredMedia>, InventoryError> {
    let inventory = collect_structured_media(article)?;
    let mut required = BTreeMap::new();
    for index in inventory.referenced_media {
        let digest = article.media[index]
            .sha256
            .as_deref()
            .ok_or(InventoryError::MissingDigest(index))?;
        let bytes = store
            .read_verified(digest)
            .map_err(|_| InventoryError::UnverifiedObject(index))?;
        required.insert(digest.to_owned(), bytes.len() as u64);
    }
    Ok(required
        .into_iter()
        .map(|(digest, bytes)| RequiredMedia { digest, bytes })
        .collect())
}
