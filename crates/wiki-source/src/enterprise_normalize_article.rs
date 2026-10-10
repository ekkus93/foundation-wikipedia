//! Construct a revision-bound Article from verified Enterprise companions.
use super::enterprise_normalize::{digest_article, CanonicalEnterpriseArticle, NormalizeError};
use super::enterprise_normalize_blocks::convert;
use super::enterprise_normalize_links::collect_links;
use super::enterprise_normalize_references::references;
use crate::enterprise::{JoinedEnterpriseArticle, StructuredTable};
use crate::enterprise_integrity::validate_joined_evidence;
use std::collections::BTreeMap;
use wiki_model::{
    Article, ArticleKey, Block, Reference, Revision, Section, ARTICLE_SCHEMA_VERSION,
};

/// Retained HTML is NOT yet sanitized for a WebView. Unresolved images
/// fail closed until a verified Commons/media join is implemented.
pub fn normalize_enterprise_article(
    joined: &JoinedEnterpriseArticle,
    is_disambiguation: bool,
) -> Result<CanonicalEnterpriseArticle, NormalizeError> {
    normalize_with_index(joined, is_disambiguation, None)
}

fn normalize_with_index(
    joined: &JoinedEnterpriseArticle,
    is_disambiguation: bool,
    link_index: Option<&BTreeMap<(String, String), ArticleKey>>,
) -> Result<CanonicalEnterpriseArticle, NormalizeError> {
    validate_joined_evidence(joined).map_err(NormalizeError::Integrity)?;
    let html = joined.rendered_html.to_ascii_lowercase();
    if html.contains("<img") || html.contains("<picture") || html.contains("<video") {
        return Err(NormalizeError::UnresolvedVisual);
    }
    let known: BTreeMap<&str, &StructuredTable> = joined
        .structured
        .tables
        .iter()
        .map(|t| (t.identifier.as_str(), t))
        .collect();
    let mut links = Vec::new();
    collect_links(
        &joined.structured.infoboxes,
        &joined.structured.page.project,
        &joined.structured.language,
        link_index,
        &mut links,
    )?;
    collect_links(
        &joined.structured.sections,
        &joined.structured.page.project,
        &joined.structured.language,
        link_index,
        &mut links,
    )?;
    let references = references(joined)?;
    let reference_index: BTreeMap<&str, &Reference> =
        references.iter().map(|r| (r.id.as_str(), r)).collect();
    let mut seen_citations = BTreeMap::new();
    let mut lead: Vec<Block> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    convert(
        &joined.structured.infoboxes,
        &mut lead,
        &mut sections,
        &known,
        &reference_index,
        &mut seen_citations,
    )?;
    convert(
        &joined.structured.sections,
        &mut lead,
        &mut sections,
        &known,
        &reference_index,
        &mut seen_citations,
    )?;
    let mut article = Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: joined.structured.page.project.clone(),
            page_id: joined.structured.page.page_id,
        },
        revision: Revision {
            revision_id: joined.structured.revision_id,
            timestamp: joined.structured.date_modified.clone(),
            content_sha256: "0".repeat(64),
        },
        title: joined.structured.name.clone(),
        display_title: joined.structured.name.clone(),
        language: joined.structured.language.clone(),
        namespace: joined.structured.namespace,
        aliases: joined.redirects.clone(),
        wikidata_id: joined.structured.wikidata_id.clone(),
        lead,
        sections,
        references,
        links,
        media: Vec::new(),
        rendered_html: joined.rendered_html.clone(),
        is_disambiguation,
    };
    article.validate().map_err(NormalizeError::Model)?;
    article.revision.content_sha256 = digest_article(&article)?;
    article.validate().map_err(NormalizeError::Model)?;
    Ok(CanonicalEnterpriseArticle {
        article,
        categories: joined.categories.clone(),
        generation_id: joined.structured.generation_id.clone(),
    })
}

/// No partial canonical import escapes if one joined page fails.
pub fn normalize_enterprise_batch(
    joined: &[JoinedEnterpriseArticle],
) -> Result<Vec<CanonicalEnterpriseArticle>, NormalizeError> {
    // Resolve internal links only against identities from the exact joined
    // generation. Cross-chunk targets require a verified global page index.
    let mut index = BTreeMap::new();
    let generation = joined
        .first()
        .map(|page| page.structured.generation_id.as_str());
    for page in joined {
        if Some(page.structured.generation_id.as_str()) != generation {
            return Err(NormalizeError::GenerationMismatch);
        }
        let key = ArticleKey {
            project: page.structured.page.project.clone(),
            page_id: page.structured.page.page_id,
        };
        for title in std::iter::once(&page.structured.name).chain(page.redirects.iter()) {
            let index_key = (key.project.clone(), title.replace('_', " "));
            if let Some(existing) = index.insert(index_key, key.clone()) {
                if existing != key {
                    return Err(NormalizeError::UnresolvedLink);
                }
            }
        }
    }
    joined
        .iter()
        .map(|page| normalize_with_index(page, false, Some(&index)))
        .collect()
}
