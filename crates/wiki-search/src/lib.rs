//! Small deterministic current-article lexical retrieval prototype.
//!
//! This indexes only one validated article revision and is not yet a BM25
//! search engine. Evidence IDs are revision-bound, not persistent bookmarks.

use std::collections::BTreeSet;
use wiki_model::{Article, Block, BlockContent, ModelError, Section};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceHit {
    pub block_id: String,
    pub revision_id: u64,
    pub heading_path: Vec<String>,
    pub excerpt: String,
    pub score: usize,
}

#[derive(Clone, Debug)]
pub struct ArticleLexicalIndex {
    revision_id: u64,
    entries: Vec<EvidenceHit>,
}

impl ArticleLexicalIndex {
    pub fn build(article: &Article) -> Result<Self, ModelError> {
        article.validate()?;
        let mut entries = Vec::new();
        append_blocks(article, &article.lead, &[], &[], &mut entries);
        for section in &article.sections {
            append_section(article, section, &[], &[], &mut entries);
        }
        Ok(Self {
            revision_id: article.revision.revision_id,
            entries,
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<EvidenceHit> {
        let terms: BTreeSet<String> = tokenize(query).into_iter().take(32).collect();
        if terms.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut results = Vec::new();
        for entry in &self.entries {
            let text_terms: BTreeSet<String> = tokenize(&entry.excerpt).into_iter().collect();
            let heading_terms: BTreeSet<String> = entry
                .heading_path
                .iter()
                .flat_map(|heading| tokenize(heading))
                .collect();
            let score = terms.iter().fold(0usize, |acc, term| {
                acc + usize::from(text_terms.contains(term))
                    + 4 * usize::from(heading_terms.contains(term))
            });
            if score > 0 {
                let mut hit = entry.clone();
                hit.score = score;
                results.push(hit);
            }
        }
        results.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.block_id.cmp(&b.block_id))
        });
        results.truncate(limit.min(100));
        results
    }

    pub fn contains_evidence(&self, hit: &EvidenceHit) -> bool {
        hit.revision_id == self.revision_id
            && self
                .entries
                .iter()
                .any(|entry| entry.block_id == hit.block_id)
    }
}

fn append_section(
    article: &Article,
    section: &Section,
    path: &[u32],
    headings: &[String],
    entries: &mut Vec<EvidenceHit>,
) {
    let mut path = path.to_vec();
    path.push(section.ordinal);
    let mut headings = headings.to_vec();
    headings.push(section.heading.clone());
    append_blocks(article, &section.blocks, &path, &headings, entries);
    for child in &section.subsections {
        append_section(article, child, &path, &headings, entries);
    }
}

fn append_blocks(
    article: &Article,
    blocks: &[Block],
    path: &[u32],
    headings: &[String],
    entries: &mut Vec<EvidenceHit>,
) {
    for block in blocks {
        let text = match &block.content {
            BlockContent::Paragraph(text) | BlockContent::Quote(text) => text.clone(),
            BlockContent::List(items) => items.join(" "),
            BlockContent::Table(rows) => {
                rows.iter().flatten().cloned().collect::<Vec<_>>().join(" ")
            }
            BlockContent::Math { source, .. } => source.clone(),
            BlockContent::Infobox(fields) => fields
                .iter()
                .map(|(key, value)| format!("{key} {value}"))
                .collect::<Vec<_>>()
                .join(" "),
            // HTML must be sanitized/normalized before contributing text.
            BlockContent::HtmlFallback(_) | BlockContent::Media { .. } => continue,
        };
        if tokenize(&text).is_empty() {
            continue;
        }
        entries.push(EvidenceHit {
            block_id: article
                .key
                .block_id(article.revision.revision_id, path, block.ordinal),
            revision_id: article.revision.revision_id,
            heading_path: headings.to_vec(),
            excerpt: text,
            score: 0,
        });
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiki_model::{ArticleKey, Revision, ARTICLE_SCHEMA_VERSION};

    fn sample() -> Article {
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 9,
            },
            revision: Revision {
                revision_id: 12,
                timestamp: "2026-10-09T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Physics".into(),
            display_title: "Physics".into(),
            language: "en".into(),
            wikidata_id: None,
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Physics studies matter.".into()),
            }],
            sections: vec![Section {
                ordinal: 1,
                heading: "Gravitation".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Paragraph("Gravity bends spacetime.".into()),
                }],
                subsections: vec![],
            }],
            references: vec![],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Physics</article>".into(),
            is_disambiguation: false,
        }
    }

    #[test]
    fn ranks_and_resolves_revision_scoped_evidence() {
        let index = ArticleLexicalIndex::build(&sample()).unwrap();
        let hits = index.search("gravitation spacetime", 2);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].block_id, "wkb:enwiki:9:12:1:b0");
        assert!(index.contains_evidence(&hits[0]));
        assert!(index.search("unknown", 20).is_empty());
        let mut newer = sample();
        newer.revision.revision_id = 13;
        assert!(!ArticleLexicalIndex::build(&newer)
            .unwrap()
            .contains_evidence(&hits[0]));
    }

    #[test]
    fn rejects_invalid_article_and_empty_query() {
        let mut article = sample();
        article.revision.revision_id = 0;
        assert_eq!(
            ArticleLexicalIndex::build(&article).unwrap_err(),
            ModelError::InvalidRevision
        );
        assert!(ArticleLexicalIndex::build(&sample())
            .unwrap()
            .search("", 1)
            .is_empty());
    }
}
