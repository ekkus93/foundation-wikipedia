//! Resolve internal links to exact same-generation page IDs, never by URL alone.
use super::enterprise_normalize::NormalizeError;
use crate::enterprise::StructuredPart;
use std::collections::BTreeMap;
use wiki_model::{ArticleKey, ArticleLink};

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex(*bytes.get(i + 1)?)?;
            let low = hex(*bytes.get(i + 2)?)?;
            out.push((high << 4) | low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return None;
    }
    Some(text.replace('_', " "))
}

fn resolve(
    url: &str,
    project: &str,
    language: &str,
    index: &BTreeMap<(String, String), ArticleKey>,
) -> Result<(ArticleKey, Option<String>), NormalizeError> {
    // The language tag alone does not establish the Wikimedia project.
    let base = if project == format!("{language}wiki") {
        format!("https://{language}.wikipedia.org/wiki/")
    } else if project == "simplewiki" {
        "https://simple.wikipedia.org/wiki/".to_owned()
    } else {
        String::new()
    };
    let path = url
        .strip_prefix("/wiki/")
        .or_else(|| {
            if base.is_empty() {
                None
            } else {
                url.strip_prefix(&base)
            }
        })
        .ok_or(NormalizeError::UnresolvedLink)?;
    // Query parameters can select a different revision and cannot be treated
    // as a current-revision canonical page link.
    if path.contains('?') {
        return Err(NormalizeError::UnresolvedLink);
    }
    let (title, fragment) = match path.split_once('#') {
        Some((title, fragment)) => (title, Some(fragment)),
        None => (path, None),
    };
    let title = decode(title).ok_or(NormalizeError::UnresolvedLink)?;
    let fragment = fragment
        .map(decode)
        .transpose_option()
        .ok_or(NormalizeError::UnresolvedLink)?;
    let target = index
        .get(&(project.to_owned(), title))
        .cloned()
        .ok_or(NormalizeError::UnresolvedLink)?;
    Ok((target, fragment))
}

trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(Some(value)) => Some(Some(value)),
            Some(None) => None,
        }
    }
}

pub(crate) fn collect_links(
    parts: &[StructuredPart],
    project: &str,
    language: &str,
    index: Option<&BTreeMap<(String, String), ArticleKey>>,
    out: &mut Vec<ArticleLink>,
) -> Result<(), NormalizeError> {
    for part in parts {
        for link in &part.links {
            let index = index.ok_or(NormalizeError::UnresolvedLink)?;
            let (target, fragment) = resolve(&link.url, project, language, index)?;
            let item = ArticleLink {
                label: link.text.clone(),
                target,
                fragment,
            };
            if !out.contains(&item) {
                out.push(item);
            }
        }
        collect_links(&part.has_parts, project, language, index, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_hosts_are_bound_to_the_actual_wiki_project() {
        let mut index = BTreeMap::new();
        index.insert(
            ("simplewiki".into(), "Gravity".into()),
            ArticleKey { project: "simplewiki".into(), page_id: 42 },
        );
        assert!(resolve("https://en.wikipedia.org/wiki/Gravity", "simplewiki", "en", &index).is_err());
        assert!(resolve("https://simple.wikipedia.org/wiki/Gravity", "simplewiki", "en", &index).is_ok());
    }
}
