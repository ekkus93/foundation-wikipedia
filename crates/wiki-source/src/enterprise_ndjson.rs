//! Bounded, line-attributed import of Wikimedia Enterprise NDJSON companions.
//! Never silently skip corrupt records or join different page revisions.

use std::collections::BTreeSet;
use std::io::{BufRead, Read};

use crate::enterprise::{
    join_enterprise_batch, parse_regular_companion_complete, parse_structured_article,
    EnterpriseError, JoinedEnterpriseArticle, RegularArticleCompanion, StructuredArticle,
    MAX_ENTERPRISE_RECORD_BYTES,
};
use crate::join::{DeletionMarker, PageKey};

/// An import chunk must remain bounded even if an upstream file is enormous.
/// Callers can partition verified snapshots into independently staged chunks.
pub const MAX_BATCH_RECORDS: usize = 10_000;
pub const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

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
    TooManyBytes {
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
    let mut total_bytes = 0usize;
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
        total_bytes = total_bytes
            .checked_add(count)
            .ok_or(BatchImportError::TooManyBytes {
                kind,
                limit: MAX_BATCH_BYTES,
            })?;
        if total_bytes > MAX_BATCH_BYTES {
            return Err(BatchImportError::TooManyBytes {
                kind,
                limit: MAX_BATCH_BYTES,
            });
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
    if generation_id.trim().is_empty()
        || generation_id != generation_id.trim()
        || generation_id.len() > 255
        || generation_id.starts_with('.')
        || generation_id.ends_with('.')
        || !generation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(BatchImportError::Join(EnterpriseError::InvalidGeneration));
    }
    let structured: Vec<StructuredArticle> =
        parse_lines(structured, InputKind::Structured, |line| {
            parse_structured_article(line, generation_id)
        })?;
    let regular: Vec<RegularArticleCompanion> = parse_lines(regular, InputKind::Regular, |line| {
        parse_regular_companion_complete(line, generation_id)
    })?;
    join_enterprise_batch(structured, regular, deleted).map_err(BatchImportError::Join)
}

/// Import an Enterprise NDJSON batch with an explicit authoritative project
/// and generation for BOTH live components and deletion evidence.
///
/// The legacy PageKey-only overload remains for compatibility, but must not
/// be used as proof of same-generation deletion. This API cannot authenticate
/// a caller-provided deletion feed or source generation by itself.
pub fn join_enterprise_ndjson_scoped<S: BufRead, R: BufRead>(
    structured: S,
    regular: R,
    project: &str,
    generation_id: &str,
    deleted: Vec<DeletionMarker>,
) -> Result<Vec<JoinedEnterpriseArticle>, BatchImportError> {
    if project.is_empty()
        || project.len() > 255
        || !project
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(BatchImportError::Join(EnterpriseError::InvalidIdentity));
    }
    if generation_id.is_empty()
        || generation_id.len() > 255
        || generation_id.starts_with('.')
        || generation_id.ends_with('.')
        || !generation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(BatchImportError::Join(EnterpriseError::InvalidGeneration));
    }
    let mut seen = BTreeSet::new();
    let mut tombstones = Vec::with_capacity(deleted.len());
    for marker in deleted {
        if marker.page.page_id == 0
            || marker.page.project != project
            || marker.page.project.len() > 255
        {
            return Err(BatchImportError::Join(EnterpriseError::InvalidIdentity));
        }
        if marker.generation_id != generation_id {
            return Err(BatchImportError::Join(EnterpriseError::GenerationMismatch));
        }
        if !seen.insert(marker.page.clone()) {
            return Err(BatchImportError::Join(EnterpriseError::DuplicateTombstone(
                marker.page,
            )));
        }
        tombstones.push(marker.page);
    }
    let joined = join_enterprise_ndjson(structured, regular, generation_id, tombstones)?;
    if joined
        .iter()
        .any(|record| record.structured.page.project != project)
    {
        return Err(BatchImportError::Join(EnterpriseError::IdentityMismatch));
    }
    Ok(joined)
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
    fn rejects_oversized_lines_without_reading_unbounded_input() {
        let mut input = vec![b'x'; MAX_ENTERPRISE_RECORD_BYTES + 1];
        input.push(b'\n');
        let error = join_enterprise_ndjson(
            Cursor::new(input),
            Cursor::new(Vec::<u8>::new()),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::OversizedRecord {
                kind: InputKind::Structured,
                line: 1,
            }
        );
    }

    #[test]
    fn caps_batch_size_before_accepting_an_extra_record() {
        let mut input = String::new();
        let article = structured(1, 17);
        for _ in 0..=MAX_BATCH_RECORDS {
            input.push_str(&article);
            input.push('\n');
        }
        let error = join_enterprise_ndjson(
            Cursor::new(input),
            Cursor::new(Vec::<u8>::new()),
            "20261010",
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            error,
            BatchImportError::TooManyRecords {
                kind: InputKind::Structured,
                limit: MAX_BATCH_RECORDS,
            }
        );
    }

    #[test]
    fn rejects_unsafe_generation_labels_before_parsing() {
        for generation in [
            "../outside",
            "a/b",
            ".hidden",
            "trailing.",
            "a b",
            "a".repeat(256).as_str(),
        ] {
            let result = join_enterprise_ndjson(
                Cursor::new(structured(1, 17)),
                Cursor::new(regular(1, 17)),
                generation,
                vec![],
            );
            assert_eq!(
                result,
                Err(BatchImportError::Join(EnterpriseError::InvalidGeneration))
            );
        }
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

    fn marker(project: &str, page_id: u64, generation: &str) -> DeletionMarker {
        DeletionMarker {
            page: PageKey {
                project: project.into(),
                page_id,
            },
            generation_id: generation.into(),
        }
    }

    #[test]
    fn scoped_enterprise_import_requires_matching_generation_and_project() {
        let correct = marker("enwiki", 2, "20261010");
        let joined = join_enterprise_ndjson_scoped(
            Cursor::new(structured(1, 17)),
            Cursor::new(regular(1, 17)),
            "enwiki",
            "20261010",
            vec![correct.clone()],
        )
        .unwrap();
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].structured.page.page_id, 1);

        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(structured(1, 17)),
                Cursor::new(regular(1, 17)),
                "enwiki",
                "20261010",
                vec![marker("enwiki", 2, "20261009")],
            ),
            Err(BatchImportError::Join(EnterpriseError::GenerationMismatch))
        );
        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(structured(1, 17)),
                Cursor::new(regular(1, 17)),
                "enwiki",
                "20261010",
                vec![marker("frwiki", 2, "20261010")],
            ),
            Err(BatchImportError::Join(EnterpriseError::InvalidIdentity))
        );
        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(structured(1, 17)),
                Cursor::new(regular(1, 17)),
                "frwiki",
                "20261010",
                vec![],
            ),
            Err(BatchImportError::Join(EnterpriseError::IdentityMismatch))
        );
        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(structured(1, 17)),
                Cursor::new(regular(1, 17)),
                "enwiki",
                "20261010",
                vec![correct.clone(), correct],
            ),
            Err(BatchImportError::Join(EnterpriseError::DuplicateTombstone(
                marker("enwiki", 2, "20261010").page
            )))
        );
    }

    #[test]
    fn scoped_import_rejects_live_deleted_and_invalid_tombstone_only_batches() {
        let live = marker("enwiki", 1, "20261010");
        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(structured(1, 17)),
                Cursor::new(regular(1, 17)),
                "enwiki",
                "20261010",
                vec![live.clone()],
            ),
            Err(BatchImportError::Join(
                EnterpriseError::DeletedPageConflict(live.page)
            ))
        );
        assert!(join_enterprise_ndjson_scoped(
            Cursor::new(Vec::<u8>::new()),
            Cursor::new(Vec::<u8>::new()),
            "enwiki",
            "20261010",
            vec![marker("enwiki", 1, "20261010")],
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(Vec::<u8>::new()),
                Cursor::new(Vec::<u8>::new()),
                "enwiki",
                "20261010",
                vec![marker("enwiki", 1, "20261009")],
            ),
            Err(BatchImportError::Join(EnterpriseError::GenerationMismatch))
        );
        assert_eq!(
            join_enterprise_ndjson_scoped(
                Cursor::new(Vec::<u8>::new()),
                Cursor::new(Vec::<u8>::new()),
                "enwiki",
                "../unsafe",
                vec![],
            ),
            Err(BatchImportError::Join(EnterpriseError::InvalidGeneration))
        );
    }

    #[test]
    fn snapshot_import_rejects_omitted_categories_and_redirects() {
        let source = structured(1, 17);
        let regular = regular(1, 17);
        let missing_categories = regular.replace(r#","categories":[{"name":"Science"}]"#, "");
        assert_ne!(regular, missing_categories);
        assert_eq!(
            join_enterprise_ndjson(
                Cursor::new(source.clone()),
                Cursor::new(missing_categories),
                "20261010",
                vec![],
            ),
            Err(BatchImportError::Parse {
                kind: InputKind::Regular,
                line: 1,
                error: EnterpriseError::MissingField("categories"),
            })
        );
        let missing_redirects = regular.replace(r#","redirects":[]"#, "");
        assert_ne!(regular, missing_redirects);
        assert_eq!(
            join_enterprise_ndjson(
                Cursor::new(source),
                Cursor::new(missing_redirects),
                "20261010",
                vec![],
            ),
            Err(BatchImportError::Parse {
                kind: InputKind::Regular,
                line: 1,
                error: EnterpriseError::MissingField("redirects"),
            })
        );
        assert!(join_enterprise_ndjson(
            Cursor::new(structured(1, 17)),
            Cursor::new(regular),
            "20261010",
            vec![],
        )
        .is_ok());
    }
}
