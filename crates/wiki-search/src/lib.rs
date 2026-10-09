//! Small deterministic current-article lexical retrieval prototype.
//!
//! This indexes only one validated article revision and is not yet a BM25
//! search engine. Evidence IDs are revision-bound, not persistent bookmarks.

use std::collections::{BTreeMap, BTreeSet};
use wiki_model::{Article, ArticleKey, Block, BlockContent, ModelError, Section};

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

    pub fn search_with_word_budget(
        &self,
        query: &str,
        limit: usize,
        budget: usize,
    ) -> Vec<EvidenceHit> {
        let mut used = 0;
        let mut selected = Vec::new();
        for hit in self.search(query, 100) {
            let count = tokenize(&hit.excerpt).len();
            if count <= budget.saturating_sub(used) && selected.len() < limit {
                used += count;
                selected.push(hit);
            }
        }
        selected
    }

    pub fn contains_evidence(&self, hit: &EvidenceHit) -> bool {
        hit.revision_id == self.revision_id
            && self.entries.iter().any(|entry| {
                entry.block_id == hit.block_id
                    && entry.excerpt == hit.excerpt
                    && entry.heading_path == hit.heading_path
            })
    }
}

/// Multi-article deterministic lexical retrieval. This is not yet BM25 or
/// a persisted multi-project inverted index; every article must be validated.
#[derive(Clone, Debug)]
pub struct CorpusLexicalIndex {
    articles: BTreeMap<(String, u64), ArticleLexicalIndex>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusHit {
    pub article: ArticleKey,
    pub evidence: EvidenceHit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusError {
    InvalidArticle(ModelError),
    DuplicateArticle(ArticleKey),
}

impl CorpusLexicalIndex {
    pub fn build(articles: &[Article]) -> Result<Self, CorpusError> {
        let mut entries = BTreeMap::new();
        for article in articles {
            let index = ArticleLexicalIndex::build(article).map_err(CorpusError::InvalidArticle)?;
            let key = (article.key.project.clone(), article.key.page_id);
            if entries.insert(key, index).is_some() {
                return Err(CorpusError::DuplicateArticle(article.key.clone()));
            }
        }
        Ok(Self { articles: entries })
    }

    /// Return a stable global ranking, bounded to 100 results.
    pub fn search(&self, query: &str, limit: usize) -> Vec<CorpusHit> {
        if limit == 0 {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for ((project, page_id), index) in &self.articles {
            for evidence in index.search(query, 100) {
                hits.push(CorpusHit {
                    article: ArticleKey {
                        project: project.clone(),
                        page_id: *page_id,
                    },
                    evidence,
                });
            }
        }
        hits.sort_by(|a, b| {
            b.evidence
                .score
                .cmp(&a.evidence.score)
                .then_with(|| a.article.project.cmp(&b.article.project))
                .then_with(|| a.article.page_id.cmp(&b.article.page_id))
                .then_with(|| a.evidence.block_id.cmp(&b.evidence.block_id))
        });
        hits.truncate(limit.min(100));
        hits
    }
}

/// An answer may cite only passages actually supplied to its model context.
/// Validation is revision-scoped and rejects tampered, stale or fabricated hits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CitationError {
    InvalidRetrievedEvidence(String),
    DuplicateRetrievedEvidence(String),
    UnretrievedCitation(String),
    DuplicateCitation(String),
    EmptyAnswer,
    UnsupportedAnswer,
}

/// Return only the exact supplied evidence for model-claimed citation IDs.
///
/// Checking the full index alone is insufficient: an LLM must not cite an
/// indexed passage that was never included in its prompt.
pub fn validate_claimed_citations(
    index: &ArticleLexicalIndex,
    retrieved: &[EvidenceHit],
    claimed_ids: &[String],
) -> Result<Vec<EvidenceHit>, CitationError> {
    let mut supplied = BTreeMap::new();
    for hit in retrieved {
        if !index.contains_evidence(hit) {
            return Err(CitationError::InvalidRetrievedEvidence(
                hit.block_id.clone(),
            ));
        }
        if supplied.insert(hit.block_id.as_str(), hit).is_some() {
            return Err(CitationError::DuplicateRetrievedEvidence(
                hit.block_id.clone(),
            ));
        }
    }

    let mut seen = BTreeSet::new();
    let mut validated = Vec::with_capacity(claimed_ids.len());
    for id in claimed_ids {
        if !seen.insert(id.as_str()) {
            return Err(CitationError::DuplicateCitation(id.clone()));
        }
        let hit = supplied
            .get(id.as_str())
            .ok_or_else(|| CitationError::UnretrievedCitation(id.clone()))?;
        validated.push((*hit).clone());
    }
    Ok(validated)
}

/// Structurally grounded answer after checking its exact supplied citations.
/// This enforces provenance, not factual entailment of the generated text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroundedAnswer {
    Supported {
        text: String,
        citations: Vec<EvidenceHit>,
    },
    InsufficientEvidence,
}

/// Require a cited substantive answer, or an explicit empty abstention.
/// The caller must not display an unsupported answer as source-grounded.
pub fn validate_grounded_answer(
    index: &ArticleLexicalIndex,
    retrieved: &[EvidenceHit],
    generated_text: &str,
    claimed_ids: &[String],
    abstained: bool,
) -> Result<GroundedAnswer, CitationError> {
    if abstained {
        if !generated_text.trim().is_empty() || !claimed_ids.is_empty() {
            return Err(CitationError::UnsupportedAnswer);
        }
        validate_claimed_citations(index, retrieved, claimed_ids)?;
        return Ok(GroundedAnswer::InsufficientEvidence);
    }
    if generated_text.trim().is_empty() {
        return Err(CitationError::EmptyAnswer);
    }
    if claimed_ids.is_empty() {
        return Err(CitationError::UnsupportedAnswer);
    }
    let citations = validate_claimed_citations(index, retrieved, claimed_ids)?;
    Ok(GroundedAnswer::Supported {
        text: generated_text.to_string(),
        citations,
    })
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
            namespace: 0,
            aliases: vec![],
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
