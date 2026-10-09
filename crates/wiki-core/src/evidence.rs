//! Version-specific article evidence handles.
use wiki_model::{Article, Block, ModelError, Reference, Section};

#[derive(Debug)]
pub struct EvidenceBlock<'a> {
    pub id: String,
    pub headings: Vec<String>,
    pub block: &'a Block,
}


#[derive(Debug)]
pub struct EvidenceSection<'a> {
    pub id: String,
    pub headings: Vec<String>,
    pub section: &'a Section,
}

/// Enumerate hard section anchors from the exact validated article revision.
/// A section's path is an ordered chain of sibling-local ordinals; unlike a
/// bookmark, this ID must never relocate to an unrelated future revision.
pub fn evidence_sections(article: &Article) -> Result<Vec<EvidenceSection<'_>>, ModelError> {
    article.validate()?;
    let mut out = Vec::new();
    collect_evidence_sections(article, &article.sections, &[], &[], &mut out);
    Ok(out)
}

fn collect_evidence_sections<'a>(
    article: &Article,
    sections: &'a [Section],
    path: &[u32],
    headings: &[String],
    out: &mut Vec<EvidenceSection<'a>>,
) {
    for section in sections {
        let mut new_path = path.to_vec();
        new_path.push(section.ordinal);
        let mut new_headings = headings.to_vec();
        new_headings.push(section.heading.clone());
        let mut id = format!(
            "wks:{}:{}:{}",
            article.key.project, article.key.page_id, article.revision.revision_id
        );
        for ordinal in &new_path {
            id.push(':');
            id.push_str(&ordinal.to_string());
        }
        out.push(EvidenceSection {
            id,
            headings: new_headings.clone(),
            section,
        });
        collect_evidence_sections(
            article,
            &section.subsections,
            &new_path,
            &new_headings,
            out,
        );
    }
}

/// Wikipedia reference notes are not independent verification of their URLs.
#[derive(Debug)]
pub struct EvidenceReference<'a> {
    pub id: String,
    pub reference: &'a Reference,
}

/// Enumerate validated, revision-bound reference handles.
pub fn evidence_references(article: &Article) -> Result<Vec<EvidenceReference<'_>>, ModelError> {
    article.validate()?;
    Ok(article
        .references
        .iter()
        .map(|reference| EvidenceReference {
            id: article
                .key
                .reference_id(article.revision.revision_id, &reference.id),
            reference,
        })
        .collect())
}

/// Traverse a validated article with provenance-scoped block identities.
pub fn evidence_blocks(article: &Article) -> Result<Vec<EvidenceBlock<'_>>, ModelError> {
    article.validate()?;
    let mut out = Vec::new();
    collect(article, &article.lead, &[], &[], &mut out);
    for section in &article.sections {
        collect_section(article, section, &[], &[], &mut out);
    }
    Ok(out)
}

fn collect_section<'a>(
    article: &Article,
    section: &'a Section,
    path: &[u32],
    headings: &[String],
    out: &mut Vec<EvidenceBlock<'a>>,
) {
    let mut new_path = path.to_vec();
    new_path.push(section.ordinal);
    let mut new_headings = headings.to_vec();
    new_headings.push(section.heading.clone());
    collect(article, &section.blocks, &new_path, &new_headings, out);
    for child in &section.subsections {
        collect_section(article, child, &new_path, &new_headings, out);
    }
}

fn collect<'a>(
    article: &Article,
    blocks: &'a [Block],
    path: &[u32],
    headings: &[String],
    out: &mut Vec<EvidenceBlock<'a>>,
) {
    for block in blocks {
        out.push(EvidenceBlock {
            id: article
                .key
                .block_id(article.revision.revision_id, path, block.ordinal),
            headings: headings.to_vec(),
            block,
        });
    }
}
