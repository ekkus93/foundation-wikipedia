//! Isolated adapters for Wikimedia Enterprise article and Structured Contents JSON.
//!
//! Structured Contents is beta. Parse only the documented identity and structured
//! fields we depend on, preserve reference payloads for later normalization, and
//! fail closed when required identity or recursive part structure is malformed.
//! Rendered HTML, categories and redirects intentionally come from the separate
//! regular article model and are joined only on exact project/page/revision.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::join::PageKey;

pub const MAX_ENTERPRISE_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PART_DEPTH: usize = 128;
pub const MAX_PARTS: usize = 200_000;

#[derive(Clone, Debug, PartialEq)]
pub struct StructuredArticle {
    pub page: PageKey,
    pub revision_id: u64,
    pub generation_id: String,
    pub namespace: i32,
    pub language: String,
    pub name: String,
    pub wikidata_id: Option<String>,
    pub description: Option<String>,
    pub infoboxes: Vec<StructuredPart>,
    pub sections: Vec<StructuredPart>,
    pub references: Vec<StructuredReference>,
    pub tables: Vec<StructuredTable>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartKind {
    Infobox,
    Section,
    Field,
    List,
    ListItem,
    Paragraph,
    Table,
    Image,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructuredPart {
    pub kind: PartKind,
    pub name: Option<String>,
    pub value: Option<String>,
    pub values: Vec<String>,
    pub links: Vec<StructuredLink>,
    pub citations: Vec<StructuredCitation>,
    pub table_references: Vec<StructuredTableReference>,
    /// Image objects remain beta and are retained losslessly for PACK-003.
    pub images: Vec<Value>,
    pub has_parts: Vec<StructuredPart>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredLink {
    pub url: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredCitation {
    pub identifier: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTableReference {
    pub identifier: String,
    /// Preserve the upstream decimal representation without using float equality.
    pub confidence_score: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructuredReference {
    pub identifier: String,
    /// Reference fields are still beta; preserve the entire validated object.
    pub payload: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructuredTable {
    pub identifier: String,
    pub headers: Vec<Vec<String>>,
    pub rows: Vec<Vec<String>>,
    pub confidence_score: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegularArticleCompanion {
    pub page: PageKey,
    pub revision_id: u64,
    pub generation_id: String,
    pub namespace: i32,
    pub language: String,
    pub name: String,
    /// Sanitization/normalization happens in the source pipeline, not this parser.
    pub rendered_html: String,
    pub categories: Vec<String>,
    pub redirects: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JoinedEnterpriseArticle {
    pub structured: StructuredArticle,
    pub rendered_html: String,
    pub categories: Vec<String>,
    pub redirects: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnterpriseError {
    RecordTooLarge,
    InvalidJson,
    MissingField(&'static str),
    InvalidField(&'static str),
    InvalidIdentity,
    InvalidGeneration,
    UnknownPartType(String),
    PartDepthExceeded,
    PartCountExceeded,
    DuplicateReference(String),
    DuplicateTable(String),
    DuplicateCategory(String),
    DuplicateRedirect(String),
    DuplicateStructured(PageKey),
    DuplicateRegular(PageKey),
    MissingStructured(PageKey),
    MissingRegular(PageKey),
    DeletedPageConflict(PageKey),
    DuplicateTombstone(PageKey),
    IdentityMismatch,
    RevisionMismatch,
    GenerationMismatch,
    NamespaceMismatch,
    LanguageMismatch,
}

fn parse_json(input: &str) -> Result<Value, EnterpriseError> {
    if input.len() > MAX_ENTERPRISE_RECORD_BYTES {
        return Err(EnterpriseError::RecordTooLarge);
    }
    serde_json::from_str(input).map_err(|_| EnterpriseError::InvalidJson)
}

fn object<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Map<String, Value>, EnterpriseError> {
    value
        .as_object()
        .ok_or(EnterpriseError::InvalidField(field))
}

fn required_string(
    map: &Map<String, Value>,
    field: &'static str,
) -> Result<String, EnterpriseError> {
    let value = map
        .get(field)
        .ok_or(EnterpriseError::MissingField(field))?
        .as_str()
        .ok_or(EnterpriseError::InvalidField(field))?;
    if value.trim().is_empty() || value != value.trim() {
        return Err(EnterpriseError::InvalidField(field));
    }
    Ok(value.to_owned())
}

fn optional_string(
    map: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<String>, EnterpriseError> {
    match map.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.clone())),
        Some(_) => Err(EnterpriseError::InvalidField(field)),
    }
}

fn required_u64(map: &Map<String, Value>, field: &'static str) -> Result<u64, EnterpriseError> {
    let value = map
        .get(field)
        .ok_or(EnterpriseError::MissingField(field))?
        .as_u64()
        .ok_or(EnterpriseError::InvalidField(field))?;
    if value == 0 {
        return Err(EnterpriseError::InvalidField(field));
    }
    Ok(value)
}

fn nested_identifier<'a>(
    map: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a Value, EnterpriseError> {
    let nested = map.get(field).ok_or(EnterpriseError::MissingField(field))?;
    object(nested, field)?
        .get("identifier")
        .ok_or(EnterpriseError::MissingField(field))
}

fn identity(
    map: &Map<String, Value>,
    generation_id: &str,
) -> Result<(PageKey, u64, i32, String, String), EnterpriseError> {
    if generation_id.trim().is_empty() || generation_id != generation_id.trim() {
        return Err(EnterpriseError::InvalidGeneration);
    }
    let page_id = required_u64(map, "identifier")?;
    let revision_id = nested_identifier(map, "version")?
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or(EnterpriseError::InvalidField("version.identifier"))?;
    let project = nested_identifier(map, "is_part_of")?
        .as_str()
        .ok_or(EnterpriseError::InvalidField("is_part_of.identifier"))?
        .to_owned();
    if project.is_empty()
        || !project
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(EnterpriseError::InvalidIdentity);
    }
    let namespace_i64 = nested_identifier(map, "namespace")?
        .as_i64()
        .ok_or(EnterpriseError::InvalidField("namespace.identifier"))?;
    let namespace = i32::try_from(namespace_i64)
        .map_err(|_| EnterpriseError::InvalidField("namespace.identifier"))?;
    let language = nested_identifier(map, "in_language")?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .ok_or(EnterpriseError::InvalidField("in_language.identifier"))?
        .to_owned();
    let name = required_string(map, "name")?;
    Ok((
        PageKey { project, page_id },
        revision_id,
        namespace,
        language,
        name,
    ))
}

fn parse_kind(value: &str) -> Result<PartKind, EnterpriseError> {
    match value {
        "infobox" => Ok(PartKind::Infobox),
        "section" => Ok(PartKind::Section),
        "field" => Ok(PartKind::Field),
        "list" => Ok(PartKind::List),
        "list_item" => Ok(PartKind::ListItem),
        "paragraph" => Ok(PartKind::Paragraph),
        "table" => Ok(PartKind::Table),
        "image" => Ok(PartKind::Image),
        other => Err(EnterpriseError::UnknownPartType(other.to_owned())),
    }
}

fn parse_strings(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Vec<String>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField(field))?;
    array
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned)
                .ok_or(EnterpriseError::InvalidField(field))
        })
        .collect()
}

fn parse_links(value: Option<&Value>) -> Result<Vec<StructuredLink>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField("links"))?;
    array
        .iter()
        .map(|item| {
            let map = object(item, "links")?;
            Ok(StructuredLink {
                url: required_string(map, "url")?,
                text: required_string(map, "text")?,
            })
        })
        .collect()
}

fn parse_citations(value: Option<&Value>) -> Result<Vec<StructuredCitation>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField("citations"))?;
    array
        .iter()
        .map(|item| {
            let map = object(item, "citations")?;
            Ok(StructuredCitation {
                identifier: required_string(map, "identifier")?,
                text: required_string(map, "text")?,
            })
        })
        .collect()
}

fn decimal_string(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<String>, EnterpriseError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => Ok(Some(number.to_string())),
        Some(_) => Err(EnterpriseError::InvalidField(field)),
    }
}

fn parse_table_refs(
    value: Option<&Value>,
) -> Result<Vec<StructuredTableReference>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField("table_references"))?;
    array
        .iter()
        .map(|item| {
            let map = object(item, "table_references")?;
            Ok(StructuredTableReference {
                identifier: required_string(map, "identifier")?,
                confidence_score: decimal_string(map.get("confidence_score"), "confidence_score")?,
            })
        })
        .collect()
}

fn parse_parts(
    value: Option<&Value>,
    field: &'static str,
    depth: usize,
    count: &mut usize,
) -> Result<Vec<StructuredPart>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    if depth > MAX_PART_DEPTH {
        return Err(EnterpriseError::PartDepthExceeded);
    }
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField(field))?;
    let mut output = Vec::with_capacity(array.len());
    for item in array {
        *count = count
            .checked_add(1)
            .ok_or(EnterpriseError::PartCountExceeded)?;
        if *count > MAX_PARTS {
            return Err(EnterpriseError::PartCountExceeded);
        }
        let map = object(item, field)?;
        let kind = parse_kind(&required_string(map, "type")?)?;
        let images = match map.get("images") {
            None => Vec::new(),
            Some(Value::Array(images)) => images.clone(),
            Some(_) => return Err(EnterpriseError::InvalidField("images")),
        };
        output.push(StructuredPart {
            kind,
            name: optional_string(map, "name")?,
            value: optional_string(map, "value")?,
            values: parse_strings(map.get("values"), "values")?,
            links: parse_links(map.get("links"))?,
            citations: parse_citations(map.get("citations"))?,
            table_references: parse_table_refs(map.get("table_references"))?,
            images,
            has_parts: parse_parts(map.get("has_parts"), "has_parts", depth + 1, count)?,
        });
    }
    Ok(output)
}

fn parse_references(value: Option<&Value>) -> Result<Vec<StructuredReference>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField("references"))?;
    let mut identifiers = BTreeSet::new();
    let mut output = Vec::with_capacity(array.len());
    for item in array {
        let map = object(item, "references")?;
        let identifier = required_string(map, "identifier")?;
        if !identifiers.insert(identifier.clone()) {
            return Err(EnterpriseError::DuplicateReference(identifier));
        }
        output.push(StructuredReference {
            identifier,
            payload: map.clone(),
        });
    }
    Ok(output)
}

fn parse_table_cells(
    value: &Value,
    field: &'static str,
) -> Result<Vec<Vec<String>>, EnterpriseError> {
    let rows = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField(field))?;
    rows.iter()
        .map(|row| {
            let cells = row.as_array().ok_or(EnterpriseError::InvalidField(field))?;
            cells
                .iter()
                .map(|cell| {
                    let map = object(cell, field)?;
                    match map.get("value") {
                        Some(Value::String(text)) => Ok(text.clone()),
                        Some(Value::Null) | None => Ok(String::new()),
                        Some(_) => Err(EnterpriseError::InvalidField(field)),
                    }
                })
                .collect()
        })
        .collect()
}

fn parse_tables(value: Option<&Value>) -> Result<Vec<StructuredTable>, EnterpriseError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField("tables"))?;
    let mut identifiers = BTreeSet::new();
    let mut output = Vec::with_capacity(array.len());
    for item in array {
        let map = object(item, "tables")?;
        let identifier = required_string(map, "identifier")?;
        if !identifiers.insert(identifier.clone()) {
            return Err(EnterpriseError::DuplicateTable(identifier));
        }
        let headers = match map.get("headers") {
            None => Vec::new(),
            Some(value) => parse_table_cells(value, "headers")?,
        };
        let rows = match map.get("rows") {
            None => Vec::new(),
            Some(value) => parse_table_cells(value, "rows")?,
        };
        output.push(StructuredTable {
            identifier,
            headers,
            rows,
            confidence_score: decimal_string(map.get("confidence_score"), "confidence_score")?,
        });
    }
    Ok(output)
}

/// Parse one Structured Contents object from on-demand JSON or snapshot NDJSON.
pub fn parse_structured_article(
    input: &str,
    generation_id: &str,
) -> Result<StructuredArticle, EnterpriseError> {
    let root = parse_json(input)?;
    let map = object(&root, "structured_content")?;
    let (page, revision_id, namespace, language, name) = identity(map, generation_id)?;
    let wikidata_id = match map.get("main_entity") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let entity = object(value, "main_entity")?;
            let id = required_string(entity, "identifier")?;
            if !id.starts_with('Q')
                || id[1..].is_empty()
                || !id[1..].bytes().all(|b| b.is_ascii_digit())
            {
                return Err(EnterpriseError::InvalidField("main_entity.identifier"));
            }
            Some(id)
        }
    };
    let mut part_count = 0;
    Ok(StructuredArticle {
        page,
        revision_id,
        generation_id: generation_id.to_owned(),
        namespace,
        language,
        name,
        wikidata_id,
        description: optional_string(map, "description")?,
        infoboxes: parse_parts(map.get("infoboxes"), "infoboxes", 0, &mut part_count)?,
        sections: parse_parts(map.get("sections"), "sections", 0, &mut part_count)?,
        references: parse_references(map.get("references"))?,
        tables: parse_tables(map.get("tables"))?,
    })
}

fn parse_string_array(
    map: &Map<String, Value>,
    field: &'static str,
) -> Result<Vec<String>, EnterpriseError> {
    let Some(value) = map.get(field) else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(EnterpriseError::InvalidField(field))?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let text = if let Some(text) = item.as_str() {
            text.to_owned()
        } else if let Some(item_map) = item.as_object() {
            item_map
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| item_map.get("identifier").and_then(Value::as_str))
                .ok_or(EnterpriseError::InvalidField(field))?
                .to_owned()
        } else {
            return Err(EnterpriseError::InvalidField(field));
        };
        if text.trim().is_empty() {
            return Err(EnterpriseError::InvalidField(field));
        }
        if !seen.insert(text.clone()) {
            return Err(match field {
                "categories" => EnterpriseError::DuplicateCategory(text),
                "redirects" => EnterpriseError::DuplicateRedirect(text),
                _ => EnterpriseError::InvalidField(field),
            });
        }
        out.push(text);
    }
    Ok(out)
}

/// Parse the regular Wikimedia Enterprise article record used to provide the
/// fields deliberately omitted from Structured Contents.
pub fn parse_regular_companion(
    input: &str,
    generation_id: &str,
) -> Result<RegularArticleCompanion, EnterpriseError> {
    let root = parse_json(input)?;
    let map = object(&root, "article")?;
    let (page, revision_id, namespace, language, name) = identity(map, generation_id)?;
    let article_body = object(
        map.get("article_body")
            .ok_or(EnterpriseError::MissingField("article_body"))?,
        "article_body",
    )?;
    let rendered_html = required_string(article_body, "html")?;
    Ok(RegularArticleCompanion {
        page,
        revision_id,
        generation_id: generation_id.to_owned(),
        namespace,
        language,
        name,
        rendered_html,
        categories: parse_string_array(map, "categories")?,
        redirects: parse_string_array(map, "redirects")?,
    })
}

/// Join beta Structured Contents with the regular article payload only when all
/// identity/provenance fields match exactly.
pub fn join_enterprise_article(
    structured: StructuredArticle,
    regular: RegularArticleCompanion,
) -> Result<JoinedEnterpriseArticle, EnterpriseError> {
    if structured.page != regular.page || structured.name != regular.name {
        return Err(EnterpriseError::IdentityMismatch);
    }
    if structured.revision_id != regular.revision_id {
        return Err(EnterpriseError::RevisionMismatch);
    }
    if structured.generation_id != regular.generation_id {
        return Err(EnterpriseError::GenerationMismatch);
    }
    if structured.namespace != regular.namespace {
        return Err(EnterpriseError::NamespaceMismatch);
    }
    if structured.language != regular.language {
        return Err(EnterpriseError::LanguageMismatch);
    }
    Ok(JoinedEnterpriseArticle {
        structured,
        rendered_html: regular.rendered_html,
        categories: regular.categories,
        redirects: regular.redirects,
    })
}

fn valid_page_key(page: &PageKey) -> bool {
    page.page_id != 0
        && !page.project.is_empty()
        && page
            .project
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Join complete Structured Contents and regular-article collections in stable
/// project/page-ID order. Duplicate, missing, or deleted live records fail
/// closed instead of silently selecting one source record.
pub fn join_enterprise_batch(
    structured: Vec<StructuredArticle>,
    regular: Vec<RegularArticleCompanion>,
    deleted: Vec<PageKey>,
) -> Result<Vec<JoinedEnterpriseArticle>, EnterpriseError> {
    let mut tombstones = BTreeSet::new();
    for page in deleted {
        if !valid_page_key(&page) {
            return Err(EnterpriseError::InvalidIdentity);
        }
        if !tombstones.insert(page.clone()) {
            return Err(EnterpriseError::DuplicateTombstone(page));
        }
    }

    let mut structured_by_page = BTreeMap::new();
    for article in structured {
        if !valid_page_key(&article.page) {
            return Err(EnterpriseError::InvalidIdentity);
        }
        if tombstones.contains(&article.page) {
            return Err(EnterpriseError::DeletedPageConflict(article.page));
        }
        let page = article.page.clone();
        if structured_by_page.insert(page.clone(), article).is_some() {
            return Err(EnterpriseError::DuplicateStructured(page));
        }
    }

    let mut regular_by_page = BTreeMap::new();
    for article in regular {
        if !valid_page_key(&article.page) {
            return Err(EnterpriseError::InvalidIdentity);
        }
        if tombstones.contains(&article.page) {
            return Err(EnterpriseError::DeletedPageConflict(article.page));
        }
        let page = article.page.clone();
        if regular_by_page.insert(page.clone(), article).is_some() {
            return Err(EnterpriseError::DuplicateRegular(page));
        }
    }

    let mut joined = Vec::with_capacity(structured_by_page.len());
    for (page, structured) in structured_by_page {
        let regular = regular_by_page
            .remove(&page)
            .ok_or_else(|| EnterpriseError::MissingRegular(page.clone()))?;
        joined.push(join_enterprise_article(structured, regular)?);
    }
    if let Some((page, _)) = regular_by_page.into_iter().next() {
        return Err(EnterpriseError::MissingStructured(page));
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structured_json() -> String {
        r#"{
          "name":"Gravity","identifier":42,
          "version":{"identifier":99},
          "is_part_of":{"identifier":"enwiki"},
          "in_language":{"identifier":"en"},
          "namespace":{"identifier":0},
          "main_entity":{"identifier":"Q1140"},
          "description":"natural phenomenon",
          "infoboxes":[{
            "name":"Infobox physics","type":"infobox",
            "has_parts":[{"name":"Field","type":"field","value":"Physics"}]
          }],
          "sections":[{
            "name":"Theory","type":"section",
            "has_parts":[{
              "type":"paragraph","value":"Gravity bends spacetime.",
              "links":[{"url":"https://en.wikipedia.org/wiki/Spacetime","text":"spacetime"}],
              "citations":[{"identifier":"cite_note-1","text":"[1]"}]
            }],
            "table_references":[{"identifier":"theory_table1","confidence_score":0.9}]
          }],
          "references":[{"identifier":"cite_note-1","url":"https://example.org/source","title":"Source"}],
          "tables":[{
            "identifier":"theory_table1",
            "headers":[[{"value":"Quantity"},{"value":"Value"}]],
            "rows":[[{"value":"g"},{"value":"9.81"}]],
            "confidence_score":0.9
          }]
        }"#
        .to_owned()
    }

    fn regular_json() -> String {
        r#"{
          "name":"Gravity","identifier":42,
          "version":{"identifier":99},
          "is_part_of":{"identifier":"enwiki"},
          "in_language":{"identifier":"en"},
          "namespace":{"identifier":0},
          "article_body":{"html":"<article><p>Gravity bends spacetime.</p></article>"},
          "categories":[{"name":"Physics"},{"name":"Gravitation"}],
          "redirects":[{"name":"Gravity (physics)"}]
        }"#
        .to_owned()
    }

    #[test]
    fn parses_recursive_structured_parts_references_and_tables() {
        let article = parse_structured_article(&structured_json(), "2026-10-01").unwrap();
        assert_eq!(article.page.page_id, 42);
        assert_eq!(article.revision_id, 99);
        assert_eq!(article.namespace, 0);
        assert_eq!(article.wikidata_id.as_deref(), Some("Q1140"));
        assert_eq!(article.infoboxes[0].has_parts[0].kind, PartKind::Field);
        assert_eq!(
            article.sections[0].has_parts[0].citations[0].identifier,
            "cite_note-1"
        );
        assert_eq!(
            article.sections[0].table_references[0].identifier,
            "theory_table1"
        );
        assert_eq!(article.references[0].identifier, "cite_note-1");
        assert_eq!(article.tables[0].headers[0], ["Quantity", "Value"]);
        assert_eq!(article.tables[0].rows[0], ["g", "9.81"]);
    }

    #[test]
    fn regular_fields_are_joined_only_at_exact_revision_and_namespace() {
        let structured = parse_structured_article(&structured_json(), "2026-10-01").unwrap();
        let regular = parse_regular_companion(&regular_json(), "2026-10-01").unwrap();
        let joined = join_enterprise_article(structured.clone(), regular.clone()).unwrap();
        assert_eq!(joined.categories, ["Physics", "Gravitation"]);
        assert_eq!(joined.redirects, ["Gravity (physics)"]);
        assert!(joined.rendered_html.starts_with("<article>"));

        let mut stale = regular.clone();
        stale.revision_id += 1;
        assert_eq!(
            join_enterprise_article(structured.clone(), stale),
            Err(EnterpriseError::RevisionMismatch)
        );
        let mut wrong_namespace = regular.clone();
        wrong_namespace.namespace = 14;
        assert_eq!(
            join_enterprise_article(structured.clone(), wrong_namespace),
            Err(EnterpriseError::NamespaceMismatch)
        );
        let mut wrong_generation = regular;
        wrong_generation.generation_id = "2026-09-01".into();
        assert_eq!(
            join_enterprise_article(structured, wrong_generation),
            Err(EnterpriseError::GenerationMismatch)
        );
    }

    #[test]
    fn unknown_beta_part_shapes_fail_closed() {
        let input =
            structured_json().replace("\"type\":\"paragraph\"", "\"type\":\"new_beta_shape\"");
        assert_eq!(
            parse_structured_article(&input, "2026-10-01"),
            Err(EnterpriseError::UnknownPartType("new_beta_shape".into()))
        );
    }

    #[test]
    fn duplicate_reference_and_table_ids_are_rejected() {
        let refs = structured_json().replace(
            "\"references\":[{\"identifier\":\"cite_note-1\",\"url\":\"https://example.org/source\",\"title\":\"Source\"}]",
            "\"references\":[{\"identifier\":\"cite_note-1\"},{\"identifier\":\"cite_note-1\"}]",
        );
        assert_eq!(
            parse_structured_article(&refs, "2026-10-01"),
            Err(EnterpriseError::DuplicateReference("cite_note-1".into()))
        );

        let tables = structured_json().replace(
            "\"tables\":[{",
            "\"tables\":[{\"identifier\":\"theory_table1\"},{",
        );
        assert_eq!(
            parse_structured_article(&tables, "2026-10-01"),
            Err(EnterpriseError::DuplicateTable("theory_table1".into()))
        );
    }

    #[test]
    fn batch_join_is_stable_and_rejects_duplicate_missing_and_deleted_pages() {
        let structured_42 = parse_structured_article(&structured_json(), "2026-10-01").unwrap();
        let regular_42 = parse_regular_companion(&regular_json(), "2026-10-01").unwrap();

        let mut structured_7 = structured_42.clone();
        structured_7.page.page_id = 7;
        structured_7.name = "Earlier page".into();
        let mut regular_7 = regular_42.clone();
        regular_7.page.page_id = 7;
        regular_7.name = "Earlier page".into();

        let joined = join_enterprise_batch(
            vec![structured_42.clone(), structured_7.clone()],
            vec![regular_42.clone(), regular_7.clone()],
            vec![],
        )
        .unwrap();
        assert_eq!(
            joined
                .iter()
                .map(|article| article.structured.page.page_id)
                .collect::<Vec<_>>(),
            vec![7, 42]
        );

        assert_eq!(
            join_enterprise_batch(
                vec![structured_42.clone(), structured_42.clone()],
                vec![regular_42.clone()],
                vec![],
            ),
            Err(EnterpriseError::DuplicateStructured(
                structured_42.page.clone()
            ))
        );
        assert_eq!(
            join_enterprise_batch(vec![structured_42.clone()], vec![], vec![]),
            Err(EnterpriseError::MissingRegular(structured_42.page.clone()))
        );
        assert_eq!(
            join_enterprise_batch(vec![], vec![regular_42.clone()], vec![]),
            Err(EnterpriseError::MissingStructured(regular_42.page.clone()))
        );
        assert_eq!(
            join_enterprise_batch(
                vec![structured_42.clone()],
                vec![regular_42.clone()],
                vec![structured_42.page.clone()],
            ),
            Err(EnterpriseError::DeletedPageConflict(
                structured_42.page.clone()
            ))
        );
        assert_eq!(
            join_enterprise_batch(
                vec![],
                vec![],
                vec![structured_42.page.clone(), structured_42.page.clone()],
            ),
            Err(EnterpriseError::DuplicateTombstone(
                structured_42.page.clone()
            ))
        );
    }

    #[test]
    fn identity_mismatch_and_duplicate_regular_members_fail_closed() {
        let structured = parse_structured_article(&structured_json(), "2026-10-01").unwrap();
        let mut wrong_page = parse_regular_companion(&regular_json(), "2026-10-01").unwrap();
        wrong_page.page.page_id += 1;
        assert_eq!(
            join_enterprise_article(structured, wrong_page),
            Err(EnterpriseError::IdentityMismatch)
        );

        let duplicate_category = regular_json().replace("\"Gravitation\"}]", "\"Physics\"}]");
        assert_eq!(
            parse_regular_companion(&duplicate_category, "2026-10-01"),
            Err(EnterpriseError::DuplicateCategory("Physics".into()))
        );
    }
}
