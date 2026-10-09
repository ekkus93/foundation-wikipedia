//! Discover image references from typed canonical article blocks.
//!
//! This is intentionally NOT an offline-completeness certificate. The final
//! renderer must separately scan sanitized HTML, CSS, fonts, MathML and
//! infobox output. Opaque HTML fallback is rejected until that scan exists.

use std::collections::BTreeSet;
use wiki_model::{Article, Block, BlockContent, ModelError, Section};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryError {
    InvalidArticle(ModelError),
    UnresolvedHtmlFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredMediaInventory {
    /// Indices into Article.media, deterministic and deduplicated.
    pub referenced_media: Vec<usize>,
    /// Must remain false until rendered HTML/CSS dependencies are inspected.
    pub rendered_dependencies_verified: bool,
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
