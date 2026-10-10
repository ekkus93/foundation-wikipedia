//! Preserve structured infobox fields and list items without flattening.
use super::enterprise_normalize::NormalizeError;
use crate::enterprise::{PartKind, StructuredPart};
use wiki_model::BlockContent;

pub(crate) fn infobox(part: &StructuredPart) -> Result<BlockContent, NormalizeError> {
    if part.value.is_some() || !part.values.is_empty() {
        return Err(NormalizeError::Unsupported(part.kind.clone()));
    }
    let mut fields = Vec::new();
    for field in &part.has_parts {
        if field.kind != PartKind::Field
            || !field.has_parts.is_empty()
            || !field.images.is_empty()
            || !field.table_references.is_empty()
        {
            return Err(NormalizeError::Unsupported(field.kind.clone()));
        }
        let name = field
            .name
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or(NormalizeError::Missing("field.name"))?;
        let value = field
            .value
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or(NormalizeError::Missing("field.value"))?;
        fields.push((name.clone(), value.clone()));
    }
    if fields.is_empty() {
        return Err(NormalizeError::Missing("infobox.fields"));
    }
    Ok(BlockContent::Infobox(fields))
}

pub(crate) fn list(part: &StructuredPart) -> Result<BlockContent, NormalizeError> {
    if part.value.is_some() || !part.table_references.is_empty() {
        return Err(NormalizeError::Unsupported(part.kind.clone()));
    }
    let mut items = part.values.clone();
    for item in &part.has_parts {
        if item.kind != PartKind::ListItem
            || !item.has_parts.is_empty()
            || !item.images.is_empty()
            || !item.table_references.is_empty()
        {
            return Err(NormalizeError::Unsupported(item.kind.clone()));
        }
        let text = item
            .value
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or(NormalizeError::Missing("list_item.value"))?;
        items.push(text.clone());
    }
    if items.is_empty() {
        return Err(NormalizeError::Missing("list.items"));
    }
    Ok(BlockContent::List(items))
}
