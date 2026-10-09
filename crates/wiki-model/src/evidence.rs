//! Revision-scoped evidence traversal; no citation may float to a new revision.
use crate::{Article, Block, ModelError, Section};

#[derive(Clone, Debug)]
pub struct EvidenceBlock<'a> {
    pub id: String,
    pub headings: Vec<String>,
    pub block: &'a Block,
}

impl Article {
    /// Return the exact block targets of this validated article revision.
    pub fn evidence_blocks(&self) -> Result<Vec<EvidenceBlock<'_>>, ModelError> {
        self.validate()?;
        let mut out = Vec::new();
        add_blocks(self, &self.lead, &[], &[], &mut out);
        for section in &self.sections {
            add_section(self, section, &[], &[], &mut out);
        }
        Ok(out)
    }
}

fn add_section<'a>(
    article: &Article,
    section: &'a Section,
    path: &[u32],
    headings: &[String],
    out: &mut Vec<EvidenceBlock<'a>>,
) {
    let mut nested_path = path.to_vec();
    nested_path.push(section.ordinal);
    let mut nested_headings = headings.to_vec();
    nested_headings.push(section.heading.clone());
    add_blocks(article, &section.blocks, &nested_path, &nested_headings, out);
    for subsection in &section.subsections {
        add_section(article, subsection, &nested_path, &nested_headings, out);
    }
}

fn add_blocks<'a>(
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
