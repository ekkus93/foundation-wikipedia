//! Version-specific article evidence handles.
use wiki_model::{Article, Block, ModelError, Section};

#[derive(Debug)]
pub struct EvidenceBlock<'a> {
    pub id: String,
    pub headings: Vec<String>,
    pub block: &'a Block,
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
