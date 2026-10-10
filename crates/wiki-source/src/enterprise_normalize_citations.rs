//! Preserve reference-backed Structured Contents citations as revision-local
//! footnotes. Unresolved page links must not be silently discarded.
use super::enterprise_normalize::NormalizeError;
use crate::enterprise::StructuredPart;
use std::collections::BTreeMap;
use wiki_model::{Block, BlockContent, Footnote, Reference};

pub(crate) fn append_citations(
    part: &StructuredPart,
    blocks: &mut Vec<Block>,
    references: &BTreeMap<&str, &Reference>,
    seen: &mut BTreeMap<String, (String, String)>,
    include_direct_children: bool,
) -> Result<(), NormalizeError> {
    if !part.links.is_empty() {
        // A URL is not a canonical ArticleKey; page-ID resolution requires
        // a verified same-generation page index before links can be emitted.
        return Err(NormalizeError::UnresolvedLink);
    }
    for citation in &part.citations {
        let reference = references
            .get(citation.identifier.as_str())
            .ok_or_else(|| NormalizeError::InvalidReference(citation.identifier.clone()))?;
        let note = (citation.text.clone(), reference.label.clone());
        if let Some(previous) = seen.get(&citation.identifier) {
            if previous != &note {
                return Err(NormalizeError::InvalidReference(citation.identifier.clone()));
            }
            continue;
        }
        let ordinal = u32::try_from(blocks.len()).map_err(|_| NormalizeError::TooManyBlocks)?;
        blocks.push(Block {
            ordinal,
            content: BlockContent::Footnote(Footnote {
                source_id: citation.identifier.clone(),
                label: note.0.clone(),
                text: note.1.clone(),
                reference_ids: vec![citation.identifier.clone()],
            }),
        });
        seen.insert(citation.identifier.clone(), note);
    }
    if include_direct_children {
        for child in &part.has_parts {
            append_citations(child, blocks, references, seen, false)?;
        }
    }
    Ok(())
}
