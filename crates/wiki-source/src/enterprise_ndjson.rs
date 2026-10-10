//! Bounded, line-attributed import of Wikimedia Enterprise NDJSON companions.
//! Never silently skip corrupt records or join different page revisions.

use std::io::{BufRead, Read};

use crate::enterprise::{
    join_enterprise_batch, parse_regular_companion, parse_structured_article, EnterpriseError,
    JoinedEnterpriseArticle, RegularArticleCompanion, StructuredArticle,
    MAX_ENTERPRISE_RECORD_BYTES,
};
use crate::join::PageKey;

/// An import chunk must remain bounded even if an upstream file is enormous.
/// Callers can partition verified snapshots into independently staged chunks.
pub const MAX_BATCH_RECORDS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Structured,
    Regular,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BatchImportError {
    Io {
        kind: InputKind,
        line: usize,
    },
    InvalidUtf8 {
        kind: InputKind,
        line: usize,
    },
    EmptyLine {
        kind: InputKind,
        line: usize,
    },
    OversizedRecord {
        kind: InputKind,
        line: usize,
    },
    TooManyRecords {
        kind: InputKind,
        limit: usize,
    },
    Parse {
        kind: InputKind,
        line: usize,
        error: EnterpriseError,
    },
    Join(EnterpriseError),
}

fn parse_lines<R: BufRead, T>(
    mut reader: R,
    kind: InputKind,
    mut parse: impl FnMut(&str) -> Result<T, EnterpriseError>,
) -> Result<Vec<T>, BatchImportError> {
    let mut result = Vec::new();
    let mut line_number = 0usize;
    loop {
        let mut bytes = Vec::new();
        // Bound allocation even for an attacker-controlled line without a newline.
        let count = (&mut reader)
            .take((MAX_ENTERPRISE_RECORD_BYTES + 2) as u64)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| BatchImportError::Io {
                kind,
                line: line_number + 1,
            })?;
        if count == 0 {
            break;
        }
        line_number += 1;
        if result.len() == MAX_BATCH_RECORDS {
            return Err(BatchImportError::TooManyRecords {
                kind,
                limit: MAX_BATCH_RECORDS,
            });
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.len() > MAX_ENTERPRISE_RECORD_BYTES {
            return Err(BatchImportError::OversizedRecord {
                kind,
                line: line_number,
            });
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| BatchImportError::InvalidUtf8 {
            kind,
            line: line_number,
        })?;
        if text.trim().is_empty() {
            return Err(BatchImportError::EmptyLine {
                kind,
                line: line_number,
            });
        }
        result.push(parse(text).map_err(|error| BatchImportError::Parse {
            kind,
            line: line_number,
            error,
        })?);
    }
    Ok(result)
}

/// Read two independently verified Enterprise NDJSON components, then perform
/// an all-or-nothing project/page/revision/generation join. Deletion markers
/// are supplied from the verified generation's deletion feed. No output is
/// returned when any line, component, or tombstone conflicts.
pub fn join_enterprise_ndjson<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    generation_id: &str,
    deleted: Vec<PageKey>,
) -> Result<Vec<JoinedEnterpriseArticle>, BatchImportError> {
    let structured: Vec<StructuredArticle> =
        parse_lines(structured, InputKind::Structured, |line| {
            parse_structured_article(line, generation_id)
        })?;
    let regular: Vec<RegularArticleCompanion> = parse_lines(regular, InputKind::Regular, |line| {
        parse_regular_companion(line, generation_id)
    })?;
    join_enterprise_batch(structured, regular, deleted).map_err(BatchImportError::Join)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn structured(page: u64, revision: u64) -> String {
        format!(
            r#"{{"name":"Example","identifier":{page},"version":{{"identifier":{revision}}},"is_part_of":{{"identifier":"enwiki"}},"in_language":{{"identifier":"en"}},"namespace":{{"identifier":0}},"date_modified":"2026-10-10T00:00:00Z"}}"#
        )
    }

    fn regular(page: u64, revision: u64) -> String {
        format!(
            r#"{{"name":"Example","identifier":{page},"version":{{"identifier":{revision}}},"is_part_of":{{"identifier":"enwiki"}},"in_language":{{"identifier":"en"}},"namespace":{{"identifier":0}},"date_modified":"2026-10-10T00:00:00Z","article_body":{{"html":"<article>Example</article>"}},"categories":[{{"name":"Science"}}],"redirects":[]}}"#
        )
    }

    #[test]
    fn joins_out_of_order_crlf_and_no_final_newline() {
        let s = format!("{}\r\n{}", structured(2, 17), structured(1, 17));
        let r = format!("{}\n{}\n", regular(1, 17), regular(2, 17));
        let joined =
            join_enterprise_ndjson(Cursor::new(s), Cursor::new(r), "20261010", vec![]).unwrap();
        assert_eq!(
            joined
                .iter()
                .map(|a| a.structured.page.page_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(joined[0].categories, ["Science"]);
    }

    #[test]
    fn refuses_revision_mismatch_and_duplicate_page() {
        let error = join_enterprise_ndjson(
            Cursor::new(structured(1, 17)),
            Cursor::new(regular(1, 18)),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::Join(EnterpriseError::RevisionMismatch)
        );
        let error = join_enterprise_ndjson(
            Cursor::new(format!("{}\n{}", structured(1, 17), structured(1, 17))),
            Cursor::new(regular(1, 17)),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::Join(EnterpriseError::DuplicateStructured(PageKey {
                project: "enwiki".into(),
                page_id: 1,
            }))
        );
    }

    #[test]
    fn reports_exact_bad_line_without_skipping_it() {
        let s = format!("{}\n{{not-json}}\n", structured(1, 17));
        let error = join_enterprise_ndjson(
            Cursor::new(s),
            Cursor::new(regular(1, 17)),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::Parse {
                kind: InputKind::Structured,
                line: 2,
                error: EnterpriseError::InvalidJson,
            }
        );
        let error = join_enterprise_ndjson(
            Cursor::new(format!("{}\n\n", structured(1, 17))),
            Cursor::new(regular(1, 17)),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::EmptyLine {
                kind: InputKind::Structured,
                line: 2,
            }
        );
    }

    #[test]
    fn rejects_invalid_utf8_and_deleted_live_records() {
        let error = join_enterprise_ndjson(
            Cursor::new(vec![0xff, b'\n']),
            Cursor::new(Vec::<u8>::new()),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::InvalidUtf8 {
                kind: InputKind::Structured,
                line: 1,
            }
        );
        let page = PageKey {
            project: "enwiki".into(),
            page_id: 1,
        };
        let error = join_enterprise_ndjson(
            Cursor::new(structured(1, 17)),
            Cursor::new(regular(1, 17)),
            "20261010",
            vec![page.clone()],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::Join(EnterpriseError::DeletedPageConflict(page))
        );
    }
}
