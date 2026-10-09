//! Relocatable, non-authoritative reading positions separate from hard citations.
//! A soft anchor never validates AI evidence and does not silently alias a
//! different article or ambiguous paragraph.

use wiki_model::{Article, ArticleKey, BlockContent, ModelError};

use crate::evidence::evidence_blocks;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoftAnchor {
    pub key: ArticleKey,
    pub original_revision: u64,
    pub original_block_id: String,
    pub section_headings: Vec<String>,
    pub original_ordinal: u32,
    pub normalized_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnchorMatch {
    Located { block_id: String, relocated: bool },
    Missing,
    Ambiguous,
    WrongArticle,
}

fn normalized_text(content: &BlockContent) -> Option<String> {
    let source = match content {
        BlockContent::Paragraph(text) | BlockContent::Quote(text) => text,
        _ => return None,
    };
    let normalized = source.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

impl SoftAnchor {
    /// Capture an exact text-bearing block in a validated article revision.
    /// The original hard evidence ID remains separately revision-bound.
    pub fn capture(article: &Article, block_id: &str) -> Result<Option<Self>, ModelError> {
        let entries = evidence_blocks(article)?;
        Ok(entries.into_iter().find_map(|entry| {
            if entry.id != block_id {
                return None;
            }
            normalized_text(&entry.block.content).map(|text| Self {
                key: article.key.clone(),
                original_revision: article.revision.revision_id,
                original_block_id: entry.id,
                section_headings: entry.headings,
                original_ordinal: entry.block.ordinal,
                normalized_text: text,
            })
        }))
    }

    /// Attempt relocation only when exact normalized text still exists.
    /// If duplicate candidates cannot be uniquely disambiguated by section
    /// and original ordinal, return Ambiguous instead of a wrong paragraph.
    pub fn relocate(&self, article: &Article) -> Result<AnchorMatch, ModelError> {
        if self.key != article.key {
            return Ok(AnchorMatch::WrongArticle);
        }

        let candidates = evidence_blocks(article)?;
        let matching: Vec<_> = candidates
            .into_iter()
            .filter(|entry| {
                normalized_text(&entry.block.content).as_deref()
                    == Some(self.normalized_text.as_str())
            })
            .collect();
        let hit = if matching.len() == 1 {
            Some(&matching[0])
        } else {
            let preferred: Vec<_> = matching
                .iter()
                .filter(|entry| {
                    entry.headings == self.section_headings
                        && entry.block.ordinal == self.original_ordinal
                })
                .collect();
            if preferred.len() == 1 {
                Some(*preferred[0])
            } else {
                None
            }
        };

        match hit {
            Some(entry) => Ok(AnchorMatch::Located {
                block_id: entry.id.clone(),
                relocated: entry.id != self.original_block_id,
            }),
            None if matching.is_empty() => Ok(AnchorMatch::Missing),
            None => Ok(AnchorMatch::Ambiguous),
        }
    }
}
