//! Exact-revision evidence references and best-effort reading anchors.
//! Model text is untrusted and must be sanitized before HTML rendering.

use crate::{Article, ArticleKey, Block, BlockContent, ModelError, Section};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceHandle {
    pub id: String,
    pub revision_id: u64,
    pub section_path: Vec<u32>,
    pub heading_path: Vec<String>,
    pub ordinal: u32,
    pub excerpt: String,
}

#[derive(Clone, Debug)]
pub struct EvidenceIndex {
    key: ArticleKey,
    revision_id: u64,
    blocks: BTreeMap<String, EvidenceHandle>,
}

impl EvidenceIndex {
    pub fn build(article: &Article) -> Result<Self, ModelError> {
        article.validate()?;
        let mut index = Self {
            key: article.key.clone(),
            revision_id: article.revision.revision_id,
            blocks: BTreeMap::new(),
        };
        index.append_blocks(&[], &[], &article.lead);
        for section in &article.sections {
            index.append_section(&[], &[], section);
        }
        Ok(index)
    }

    /// Never resolve an old revision's citation against a newer article.
    pub fn resolve(
        &self,
        key: &ArticleKey,
        revision_id: u64,
        id: &str,
    ) -> Option<&EvidenceHandle> {
        if *key != self.key || revision_id != self.revision_id {
            return None;
        }
        self.blocks.get(id)
    }

    pub fn handles(&self) -> impl Iterator<Item = &EvidenceHandle> {
        self.blocks.values()
    }

    /// A quote/heading anchor is not authoritative evidence and may be lost.
    /// Ambiguous relocation MUST NOT guess at a new paragraph.
    pub fn relocate(&self, anchor: &SoftReadingAnchor) -> AnchorRelocation {
        if anchor.key != self.key || anchor.quote.is_empty() {
            return AnchorRelocation::Missing;
        }
        let mut matches = self.blocks.values().filter(|block| {
            block.excerpt == anchor.quote && block.heading_path == anchor.heading_path
        });
        match (matches.next(), matches.next()) {
            (Some(block), None) => AnchorRelocation::Found(block.id.clone()),
            (Some(_), Some(_)) => AnchorRelocation::Ambiguous,
            _ => AnchorRelocation::Missing,
        }
    }

    fn append_blocks(&mut self, path: &[u32], headings: &[String], blocks: &[Block]) {
        for block in blocks {
            let Some(excerpt) = block_text(&block.content) else {
                continue;
            };
            let id = self.key.block_id(self.revision_id, path, block.ordinal);
            self.blocks.insert(
                id.clone(),
                EvidenceHandle {
                    id,
                    revision_id: self.revision_id,
                    section_path: path.to_vec(),
                    heading_path: headings.to_vec(),
                    ordinal: block.ordinal,
                    excerpt,
                },
            );
        }
    }

    fn append_section(&mut self, parent: &[u32], headings: &[String], section: &Section) {
        let mut path = parent.to_vec();
        path.push(section.ordinal);
        let mut titles = headings.to_vec();
        titles.push(section.heading.clone());
        self.append_blocks(&path, &titles, &section.blocks);
        for child in &section.subsections {
            self.append_section(&path, &titles, child);
        }
    }
}

fn block_text(content: &BlockContent) -> Option<String> {
    let text = match content {
        BlockContent::Paragraph(text) | BlockContent::Quote(text) => text.clone(),
        BlockContent::List(items) => items.join(" "),
        BlockContent::Table(rows) => {
            rows.iter().flatten().cloned().collect::<Vec<_>>().join(" ")
        }
        BlockContent::Math { source, .. } => source.clone(),
        BlockContent::Infobox(fields) => fields
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join(" "),
        BlockContent::HtmlFallback(_) | BlockContent::Media { .. } => return None,
    };
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoftReadingAnchor {
    pub key: ArticleKey,
    pub heading_path: Vec<String>,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnchorRelocation {
    Found(String),
    Ambiguous,
    Missing,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Revision, ARTICLE_SCHEMA_VERSION};

    fn fixture() -> Article {
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 101,
            },
            revision: Revision {
                revision_id: 10,
                timestamp: "2026-10-09T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Science".into(),
            display_title: "Science".into(),
            language: "en".into(),
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Science is systematic.".into()),
            }],
            sections: vec![Section {
                ordinal: 1,
                heading: "Method".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Paragraph("Observations and experiments.".into()),
                }],
                subsections: vec![],
            }],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Science</article>".into(),
            is_disambiguation: false,
        }
    }

    #[test]
    fn citation_looks_up_only_same_page_and_revision() {
        let article = fixture();
        let index = EvidenceIndex::build(&article).unwrap();
        let expected = "wkb:enwiki:101:10:1:b0";
        assert!(index.resolve(&article.key, 10, expected).is_some());
        assert!(index.resolve(&article.key, 11, expected).is_none());
        assert!(index.resolve(&article.key, 10, "nonexistent").is_none());
        let other = ArticleKey {
            project: "enwiki".into(),
            page_id: 102,
        };
        assert!(index.resolve(&other, 10, expected).is_none());
    }

    #[test]
    fn soft_anchor_requires_unambiguous_exact_match() {
        let article = fixture();
        let index = EvidenceIndex::build(&article).unwrap();
        let anchor = SoftReadingAnchor {
            key: article.key,
            heading_path: vec!["Method".into()],
            quote: "Observations and experiments.".into(),
        };
        assert_eq!(
            index.relocate(&anchor),
            AnchorRelocation::Found("wkb:enwiki:101:10:1:b0".into())
        );
        let mut updated = fixture();
        updated.sections[0].blocks.push(Block {
            ordinal: 1,
            content: BlockContent::Paragraph("Observations and experiments.".into()),
        });
        assert_eq!(
            EvidenceIndex::build(&updated).unwrap().relocate(&anchor),
            AnchorRelocation::Ambiguous
        );
    }
}
