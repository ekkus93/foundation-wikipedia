//! Construct a revision-bound Article from verified Enterprise companions.
use super::enterprise_normalize::{digest_article, CanonicalEnterpriseArticle, NormalizeError};
use super::enterprise_normalize_blocks::convert;
use super::enterprise_normalize_html::sanitize_rendered_html;
use super::enterprise_normalize_links::collect_links;
use super::enterprise_normalize_references::references;
use crate::enterprise::{JoinedEnterpriseArticle, StructuredTable};
use crate::enterprise_integrity::validate_joined_evidence;
use std::collections::{BTreeMap, BTreeSet};
use wiki_model::{
    Article, ArticleKey, Block, Reference, Revision, Section, ARTICLE_SCHEMA_VERSION,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnterpriseLinkIdentity {
    pub project: String,
    pub page_id: u64,
    pub generation_id: String,
    pub title: String,
    pub redirects: Vec<String>,
}

impl EnterpriseLinkIdentity {
    pub fn from_joined(page: &JoinedEnterpriseArticle) -> Self {
        Self {
            project: page.structured.page.project.clone(),
            page_id: page.structured.page.page_id,
            generation_id: page.structured.generation_id.clone(),
            title: page.structured.name.clone(),
            redirects: page.redirects.clone(),
        }
    }
}

/// Snapshot-wide title/redirect resolver bound to exactly one project and
/// generation. Build this from the verified identity pass, then reuse it while
/// canonical records are normalized in bounded chunks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnterpriseLinkIndex {
    project: String,
    generation_id: String,
    by_title: BTreeMap<(String, String), ArticleKey>,
    by_page: BTreeMap<u64, (String, Vec<String>)>,
}

impl EnterpriseLinkIndex {
    pub fn build(identities: &[EnterpriseLinkIdentity]) -> Result<Self, NormalizeError> {
        let first = identities
            .first()
            .ok_or(NormalizeError::Missing("link index identity"))?;
        let project = first.project.clone();
        let generation_id = first.generation_id.clone();
        let mut by_title = BTreeMap::new();
        let mut seen_pages = BTreeSet::new();
        let mut by_page = BTreeMap::new();

        for identity in identities {
            if identity.project != project {
                return Err(NormalizeError::ProjectMismatch);
            }
            if identity.generation_id != generation_id {
                return Err(NormalizeError::GenerationMismatch);
            }
            if identity.generation_id.trim().is_empty()
                || identity.generation_id != identity.generation_id.trim()
            {
                return Err(NormalizeError::Missing("link index generation"));
            }
            let key = ArticleKey {
                project: identity.project.clone(),
                page_id: identity.page_id,
            };
            key.validate().map_err(NormalizeError::Model)?;
            if !seen_pages.insert((key.project.clone(), key.page_id)) {
                return Err(NormalizeError::DuplicatePage(key));
            }
            by_page.insert(
                identity.page_id,
                (identity.title.clone(), identity.redirects.clone()),
            );
            for title in std::iter::once(&identity.title).chain(identity.redirects.iter()) {
                if title.trim().is_empty() || title != title.trim() {
                    return Err(NormalizeError::Missing("link index title"));
                }
                let index_key = (key.project.clone(), title.replace('_', " "));
                if let Some(existing) = by_title.insert(index_key, key.clone()) {
                    if existing != key {
                        return Err(NormalizeError::UnresolvedLink);
                    }
                }
            }
        }

        Ok(Self {
            project,
            generation_id,
            by_title,
            by_page,
        })
    }

    fn validate_page(&self, page: &JoinedEnterpriseArticle) -> Result<(), NormalizeError> {
        if page.structured.page.project != self.project {
            return Err(NormalizeError::ProjectMismatch);
        }
        if page.structured.generation_id != self.generation_id {
            return Err(NormalizeError::GenerationMismatch);
        }
        let Some((title, redirects)) = self.by_page.get(&page.structured.page.page_id) else {
            return Err(NormalizeError::UnresolvedLink);
        };
        if title != &page.structured.name || redirects != &page.redirects {
            return Err(NormalizeError::UnresolvedLink);
        }
        Ok(())
    }
}

pub fn normalize_enterprise_article_with_link_index(
    joined: &JoinedEnterpriseArticle,
    is_disambiguation: bool,
    link_index: &EnterpriseLinkIndex,
) -> Result<CanonicalEnterpriseArticle, NormalizeError> {
    link_index.validate_page(joined)?;
    normalize_with_index(joined, is_disambiguation, Some(&link_index.by_title))
}

/// Rendered HTML is sanitized at the source boundary. Unresolved images
/// still fail closed until a verified Commons/media join is implemented.
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
    // Reject unresolved embedded media and rich visual markup before HTML
    // sanitization can silently remove required offline article content.
    let html = joined.rendered_html.to_ascii_lowercase();
    const UNRESOLVED_VISUAL_TAGS: [&str; 12] = [
        "<img", "<picture", "<video", "<audio", "<source", "<track", "<svg", "<math", "<canvas",
        "<iframe", "<object", "<embed",
    ];
    if UNRESOLVED_VISUAL_TAGS.iter().any(|tag| html.contains(tag)) {
        return Err(NormalizeError::UnresolvedVisual);
    }
    let rendered_html = sanitize_rendered_html(&joined.rendered_html)?;
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
        rendered_html,
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
    if joined.is_empty() {
        return Ok(Vec::new());
    }
    let identities = joined
        .iter()
        .map(EnterpriseLinkIdentity::from_joined)
        .collect::<Vec<_>>();
    let index = EnterpriseLinkIndex::build(&identities)?;
    joined
        .iter()
        .map(|page| normalize_enterprise_article_with_link_index(page, false, &index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enterprise::{PartKind, StructuredArticle, StructuredLink, StructuredPart};
    use crate::join::PageKey;

    fn joined(project: &str, page_id: u64) -> JoinedEnterpriseArticle {
        JoinedEnterpriseArticle {
            structured: StructuredArticle {
                page: PageKey {
                    project: project.into(),
                    page_id,
                },
                revision_id: 100,
                date_modified: "2026-10-10T00:00:00Z".into(),
                generation_id: "20261010".into(),
                namespace: 0,
                language: "en".into(),
                name: format!("Article {page_id}"),
                wikidata_id: None,
                description: None,
                infoboxes: vec![],
                sections: vec![],
                references: vec![],
                tables: vec![],
            },
            rendered_html: "<article>Verified companion</article>".into(),
            categories: vec![],
            redirects: vec![],
        }
    }

    #[test]
    fn canonical_batch_rejects_cross_project_generation_collision() {
        let pages = [joined("enwiki", 42), joined("simplewiki", 43)];
        assert_eq!(
            normalize_enterprise_batch(&pages),
            Err(NormalizeError::ProjectMismatch)
        );
    }

    #[test]
    fn canonical_batch_rejects_duplicate_page_even_with_same_identity() {
        let page = joined("enwiki", 42);
        assert_eq!(
            normalize_enterprise_batch(&[page.clone(), page]),
            Err(NormalizeError::DuplicatePage(ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            }))
        );
    }

    #[test]
    fn snapshot_link_index_resolves_targets_outside_the_current_chunk() {
        let mut source = joined("enwiki", 42);
        source.structured.name = "Source".into();
        source.structured.sections.push(StructuredPart {
            kind: PartKind::Paragraph,
            name: None,
            value: Some("See the target article.".into()),
            values: vec![],
            links: vec![StructuredLink {
                url: "/wiki/Target".into(),
                text: "Target".into(),
            }],
            citations: vec![],
            table_references: vec![],
            images: vec![],
            has_parts: vec![],
        });
        let mut target = joined("enwiki", 43);
        target.structured.name = "Target".into();
        target.redirects.push("Target_alias".into());

        let identities = [&source, &target]
            .into_iter()
            .map(EnterpriseLinkIdentity::from_joined)
            .collect::<Vec<_>>();
        let index = EnterpriseLinkIndex::build(&identities).unwrap();

        let normalized =
            normalize_enterprise_article_with_link_index(&source, false, &index).unwrap();
        assert_eq!(normalized.article.links.len(), 1);
        assert_eq!(normalized.article.links[0].target.page_id, 43);
        assert_eq!(normalized.article.links[0].target.project, "enwiki");
    }

    #[test]
    fn link_index_rejects_unverified_identity_metadata() {
        let original = EnterpriseLinkIdentity::from_joined(&joined("enwiki", 42));

        let mut invalid = original.clone();
        invalid.page_id = 0;
        assert_eq!(
            EnterpriseLinkIndex::build(&[invalid]),
            Err(NormalizeError::Model(
                wiki_model::ModelError::InvalidIdentity
            ))
        );

        let mut invalid = original.clone();
        invalid.generation_id = " ".into();
        assert_eq!(
            EnterpriseLinkIndex::build(&[invalid]),
            Err(NormalizeError::Missing("link index generation"))
        );

        let mut invalid = original.clone();
        invalid.title = " ".into();
        assert_eq!(
            EnterpriseLinkIndex::build(&[invalid]),
            Err(NormalizeError::Missing("link index title"))
        );

        let mut invalid = original;
        invalid.redirects.push(" malformed ".into());
        assert_eq!(
            EnterpriseLinkIndex::build(&[invalid]),
            Err(NormalizeError::Missing("link index title"))
        );
    }

    #[test]
    fn snapshot_link_index_rejects_unindexed_or_changed_page_identity() {
        let page = joined("enwiki", 42);
        let index =
            EnterpriseLinkIndex::build(&[EnterpriseLinkIdentity::from_joined(&page)]).unwrap();

        assert_eq!(
            normalize_enterprise_article_with_link_index(&joined("enwiki", 43), false, &index),
            Err(NormalizeError::UnresolvedLink)
        );

        let mut renamed = page.clone();
        renamed.structured.name = "Changed title".into();
        assert_eq!(
            normalize_enterprise_article_with_link_index(&renamed, false, &index),
            Err(NormalizeError::UnresolvedLink)
        );

        let mut redirected = page;
        redirected.redirects.push("Unindexed alias".into());
        assert_eq!(
            normalize_enterprise_article_with_link_index(&redirected, false, &index),
            Err(NormalizeError::UnresolvedLink)
        );
    }

    #[test]
    fn snapshot_link_index_rejects_cross_generation_use() {
        let page = joined("enwiki", 42);
        let index =
            EnterpriseLinkIndex::build(&[EnterpriseLinkIdentity::from_joined(&page)]).unwrap();
        let mut stale = page;
        stale.structured.generation_id = "20261011".into();
        assert_eq!(
            normalize_enterprise_article_with_link_index(&stale, false, &index),
            Err(NormalizeError::GenerationMismatch)
        );
    }
}
