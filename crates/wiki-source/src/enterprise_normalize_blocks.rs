//! Loss-aware Structured Contents block conversion.
use std::collections::BTreeMap;
use wiki_model::{Block, BlockContent, Section};
use crate::enterprise::{PartKind, StructuredPart, StructuredTable};
use super::enterprise_normalize::NormalizeError;

fn push(blocks: &mut Vec<Block>, content: BlockContent) -> Result<(), NormalizeError> {
    let ordinal = u32::try_from(blocks.len()).map_err(|_| NormalizeError::TooManyBlocks)?;
    blocks.push(Block { ordinal, content });
    Ok(())
}
fn value(part: &StructuredPart) -> Result<String, NormalizeError> {
    if !part.values.is_empty() || !part.has_parts.is_empty() {
        return Err(NormalizeError::Unsupported(part.kind.clone()));
    }
    part.value.as_ref().filter(|s| !s.trim().is_empty()).cloned()
        .ok_or(NormalizeError::Missing("part.value"))
}
fn append_tables(
    part: &StructuredPart, blocks: &mut Vec<Block>,
    known: &BTreeMap<&str, &StructuredTable>,
) -> Result<(), NormalizeError> {
    for reference in &part.table_references {
        let table = known.get(reference.identifier.as_str())
            .ok_or_else(|| NormalizeError::InvalidTable(reference.identifier.clone()))?;
        let mut rows = table.headers.clone();
        rows.extend(table.rows.iter().cloned());
        if rows.is_empty() || rows.iter().any(Vec::is_empty) {
            return Err(NormalizeError::InvalidTable(table.identifier.clone()));
        }
        push(blocks, BlockContent::Table(rows))?;
    }
    Ok(())
}
pub(crate) fn convert(
    parts: &[StructuredPart], blocks: &mut Vec<Block>, sections: &mut Vec<Section>,
    known: &BTreeMap<&str, &StructuredTable>,
) -> Result<(), NormalizeError> {
    for part in parts {
        if !part.images.is_empty() || part.kind == PartKind::Image {
            return Err(NormalizeError::UnresolvedVisual);
        }
        match &part.kind {
            PartKind::Section => {
                let heading = part.name.as_ref().filter(|s| !s.trim().is_empty())
                    .ok_or(NormalizeError::Missing("section.name"))?.clone();
                if part.value.is_some() || !part.values.is_empty() {
                    return Err(NormalizeError::Unsupported(part.kind.clone()));
                }
                let mut child_blocks = Vec::new();
                let mut child_sections = Vec::new();
                convert(&part.has_parts, &mut child_blocks, &mut child_sections, known)?;
                append_tables(part, &mut child_blocks, known)?;
                let ordinal = u32::try_from(sections.len() + 1)
                    .map_err(|_| NormalizeError::TooManyBlocks)?;
                sections.push(Section { ordinal, heading, blocks: child_blocks, subsections: child_sections });
            }
            PartKind::Paragraph => {
                push(blocks, BlockContent::Paragraph(value(part)?))?;
                append_tables(part, blocks, known)?;
            }
            PartKind::Table => {
                if part.value.is_some() || !part.values.is_empty()
                    || !part.has_parts.is_empty() || part.table_references.is_empty()
                {
                    return Err(NormalizeError::Unsupported(part.kind.clone()));
                }
                append_tables(part, blocks, known)?;
            }
            _ => return Err(NormalizeError::Unsupported(part.kind.clone())),
        }
    }
    Ok(())
}
