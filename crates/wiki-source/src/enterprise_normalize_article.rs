//! Construct a revision-bound Article from verified Enterprise companions.
use super::enterprise_normalize::{digest_article, CanonicalEnterpriseArticle, NormalizeError};
use super::enterprise_normalize_blocks::convert;
use super::enterprise_normalize_references::references;
use crate::enterprise::{JoinedEnterpriseArticle, StructuredTable};
use crate::enterprise_integrity::validate_joined_evidence;
use std::collections::BTreeMap;
use wiki_model::{Article, ArticleKey, Block, Revision, Section, ARTICLE_SCHEMA_VERSION};

/// Retained HTML is NOT yet sanitized for a WebView. Unresolved images
/// fail closed until a verified Commons/media join is implemented.
pub fn normalize_enterprise_article(
    joined: &JoinedEnterpriseArticle,
    is_disambiguation: bool,
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
    let mut lead: Vec<Block> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();
    convert(
        &joined.structured.infoboxes,
        &mut lead,
        &mut sections,
        &known,
    )?;
    convert(
        &joined.structured.sections,
        &mut lead,
        &mut sections,
        &known,
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
        references: references(joined)?,
        links: Vec::new(),
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
    joined
        .iter()
        .map(|page| normalize_enterprise_article(page, false))
        .collect()
}
