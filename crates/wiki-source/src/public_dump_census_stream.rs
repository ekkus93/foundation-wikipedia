//! Streaming page-ID census reconciliation for independently sourced exports.
//!
//! The existing slice-based census audit requires materializing both datasets.
//! This variant consumes sorted, fallible iterators with O(1) comparison state
//! and at most twelve diagnostic IDs per side. Neither matching identifiers
//! nor matching caller-provided labels authenticate Wikimedia publication,
//! prove that the two sources are independent, or qualify a rendered pack.

use std::io;

use crate::public_dump_coverage::PageCoverageDelta;
#[cfg(test)]
use crate::public_dump_coverage::PageCoverageError;

const MAX_IDS: usize = 100_000_000;
const MAX_EXAMPLES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CensusScope<'a> {
    pub project: &'a str,
    pub generation: &'a str,
    pub namespace: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CensusSide {
    Expected,
    Observed,
}

#[derive(Debug)]
pub enum StreamCoverageError {
    IncompatibleCensus,
    InvalidCensus,
    InvalidId { side: CensusSide, page_id: u64 },
    UnsortedOrDuplicate { side: CensusSide, page_id: u64 },
    TooManyPages { side: CensusSide },
    Read { side: CensusSide, source: io::Error },
    MissingOrUnexpected(PageCoverageDelta),
}

fn valid_scope(scope: CensusScope<'_>) -> bool {
    crate::valid_wiki_project(scope.project)
        && !scope.generation.is_empty()
        && scope.generation.len() <= 255
        && !scope.generation.starts_with('.')
        && !scope.generation.ends_with('.')
        && scope
            .generation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && scope.namespace >= 0
}

struct Cursor<I> {
    input: I,
    side: CensusSide,
    previous: u64,
    count: usize,
}

impl<I: Iterator<Item = io::Result<u64>>> Cursor<I> {
    fn next_id(&mut self) -> Result<Option<u64>, StreamCoverageError> {
        let Some(item) = self.input.next() else {
            return Ok(None);
        };
        let id = item.map_err(|source| StreamCoverageError::Read {
            side: self.side,
            source,
        })?;
        if id == 0 || id > i64::MAX as u64 {
            return Err(StreamCoverageError::InvalidId {
                side: self.side,
                page_id: id,
            });
        }
        if id <= self.previous {
            return Err(StreamCoverageError::UnsortedOrDuplicate {
                side: self.side,
                page_id: id,
            });
        }
        if self.count >= MAX_IDS {
            return Err(StreamCoverageError::TooManyPages { side: self.side });
        }
        self.previous = id;
        self.count += 1;
        Ok(Some(id))
    }
}

fn record_missing(delta: &mut PageCoverageDelta, id: u64) {
    delta.missing_count += 1;
    if delta.missing_examples.len() < MAX_EXAMPLES {
        delta.missing_examples.push(id);
    }
}

fn record_unexpected(delta: &mut PageCoverageDelta, id: u64) {
    delta.unexpected_count += 1;
    if delta.unexpected_examples.len() < MAX_EXAMPLES {
        delta.unexpected_examples.push(id);
    }
}

/// Reconcile two independently obtained, same-generation, same-namespace
/// sorted ID streams. Any I/O error, duplicate, invalid ID or premature EOF
/// signaled by the reader fails closed. Readers MUST surface truncation as
/// an error rather than returning normal EOF. Both streams must be nonempty.
///
/// For a production completeness claim, separately authenticate the expected
/// census, prove its generation cutoff and namespace semantics, and validate
/// that the observed IDs came from digest-verified, fully decoded dump shards.
pub fn audit_page_coverage_stream<E, O>(
    expected_scope: CensusScope<'_>,
    expected: E,
    observed_scope: CensusScope<'_>,
    observed: O,
) -> Result<PageCoverageDelta, StreamCoverageError>
where
    E: IntoIterator<Item = io::Result<u64>>,
    O: IntoIterator<Item = io::Result<u64>>,
{
    if expected_scope != observed_scope {
        return Err(StreamCoverageError::IncompatibleCensus);
    }
    if !valid_scope(expected_scope) {
        return Err(StreamCoverageError::InvalidCensus);
    }
    let mut expected = Cursor {
        input: expected.into_iter(),
        side: CensusSide::Expected,
        previous: 0,
        count: 0,
    };
    let mut observed = Cursor {
        input: observed.into_iter(),
        side: CensusSide::Observed,
        previous: 0,
        count: 0,
    };
    let mut delta = PageCoverageDelta {
        expected_count: 0,
        observed_count: 0,
        missing_count: 0,
        unexpected_count: 0,
        missing_examples: Vec::new(),
        unexpected_examples: Vec::new(),
    };
    let mut wanted = expected.next_id()?;
    let mut found = observed.next_id()?;
    loop {
        match (wanted, found) {
            (None, None) => break,
            (Some(a), Some(b)) if a == b => {
                wanted = expected.next_id()?;
                found = observed.next_id()?;
            }
            (Some(a), Some(b)) if a < b => {
                record_missing(&mut delta, a);
                wanted = expected.next_id()?;
            }
            (Some(_), Some(b)) => {
                record_unexpected(&mut delta, b);
                found = observed.next_id()?;
            }
            (Some(a), None) => {
                record_missing(&mut delta, a);
                wanted = expected.next_id()?;
            }
            (None, Some(b)) => {
                record_unexpected(&mut delta, b);
                found = observed.next_id()?;
            }
        }
    }
    delta.expected_count = expected.count;
    delta.observed_count = observed.count;
    if delta.expected_count == 0 || delta.observed_count == 0 {
        return Err(StreamCoverageError::InvalidCensus);
    }
    if delta.missing_count != 0 || delta.unexpected_count != 0 {
        return Err(StreamCoverageError::MissingOrUnexpected(delta));
    }
    Ok(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> CensusScope<'static> {
        CensusScope {
            project: "enwiki",
            generation: "2026-10-01",
            namespace: 0,
        }
    }

    fn ids(input: &[u64]) -> impl Iterator<Item = io::Result<u64>> + '_ {
        input.iter().copied().map(Ok::<_, io::Error>)
    }

    #[test]
    fn matches_large_sorted_streams_without_materializing_them() {
        let expected = (1..=25_000).map(Ok::<_, io::Error>);
        let observed = (1..=25_000).map(Ok::<_, io::Error>);
        let delta = audit_page_coverage_stream(scope(), expected, scope(), observed).unwrap();
        assert_eq!(delta.expected_count, 25_000);
        assert_eq!(delta.observed_count, 25_000);
        assert_eq!(delta.missing_count, 0);
        assert_eq!(delta.unexpected_count, 0);
    }

    #[test]
    fn diagnoses_missing_and_unexpected_ids_with_bounded_examples() {
        let result =
            audit_page_coverage_stream(scope(), ids(&[1, 3, 5, 7]), scope(), ids(&[1, 2, 7, 8]));
        let Err(StreamCoverageError::MissingOrUnexpected(delta)) = result else {
            panic!("must reject an incomplete census");
        };
        assert_eq!(delta.expected_count, 4);
        assert_eq!(delta.observed_count, 4);
        assert_eq!(delta.missing_examples, [3, 5]);
        assert_eq!(delta.unexpected_examples, [2, 8]);

        let result = audit_page_coverage_stream(
            scope(),
            (1..=100).map(Ok::<_, io::Error>),
            scope(),
            (101..=200).map(Ok::<_, io::Error>),
        );
        let Err(StreamCoverageError::MissingOrUnexpected(delta)) = result else {
            panic!("must reject disjoint inventories");
        };
        assert_eq!(delta.missing_count, 100);
        assert_eq!(delta.unexpected_count, 100);
        assert_eq!(delta.missing_examples.len(), MAX_EXAMPLES);
        assert_eq!(delta.unexpected_examples.len(), MAX_EXAMPLES);
    }

    #[test]
    fn rejects_incompatible_invalid_and_empty_censuses() {
        let other = CensusScope {
            namespace: 1,
            ..scope()
        };
        assert!(matches!(
            audit_page_coverage_stream(scope(), ids(&[1]), other, ids(&[1])),
            Err(StreamCoverageError::IncompatibleCensus)
        ));
        let bad = CensusScope {
            generation: "../wrong",
            ..scope()
        };
        assert!(matches!(
            audit_page_coverage_stream(bad, ids(&[1]), bad, ids(&[1])),
            Err(StreamCoverageError::InvalidCensus)
        ));
        for project in ["_enwiki", "enwiki_", "123wiki"] {
            let invalid = CensusScope { project, ..scope() };
            assert!(matches!(
                audit_page_coverage_stream(invalid, ids(&[1]), invalid, ids(&[1])),
                Err(StreamCoverageError::InvalidCensus)
            ));
        }
        let invalid = CensusScope {
            namespace: -1,
            ..scope()
        };
        assert!(matches!(
            audit_page_coverage_stream(invalid, ids(&[1]), invalid, ids(&[1])),
            Err(StreamCoverageError::InvalidCensus)
        ));
        assert!(matches!(
            audit_page_coverage_stream(scope(), ids(&[]), scope(), ids(&[1])),
            Err(StreamCoverageError::InvalidCensus)
        ));
        assert!(matches!(
            audit_page_coverage_stream(scope(), ids(&[1]), scope(), ids(&[])),
            Err(StreamCoverageError::InvalidCensus)
        ));
    }

    #[test]
    fn fails_closed_on_duplicate_invalid_and_unsorted_ids() {
        for input in [&[1, 1][..], &[2, 1][..]] {
            assert!(matches!(
                audit_page_coverage_stream(scope(), ids(input), scope(), ids(&[1, 2])),
                Err(StreamCoverageError::UnsortedOrDuplicate {
                    side: CensusSide::Expected,
                    ..
                })
            ));
        }
        for bad in [0, i64::MAX as u64 + 1] {
            assert!(matches!(
                audit_page_coverage_stream(scope(), ids(&[1]), scope(), ids(&[bad])),
                Err(StreamCoverageError::InvalidId {
                    side: CensusSide::Observed,
                    ..
                })
            ));
        }
    }

    #[test]
    fn read_failure_is_never_mistaken_for_normal_eof() {
        let broken = || {
            [
                Ok(1),
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated census",
                )),
            ]
        };
        assert!(matches!(
            audit_page_coverage_stream(scope(), broken(), scope(), ids(&[1])),
            Err(StreamCoverageError::Read {
                side: CensusSide::Expected,
                ..
            })
        ));
        assert!(matches!(
            audit_page_coverage_stream(scope(), ids(&[1]), scope(), broken()),
            Err(StreamCoverageError::Read {
                side: CensusSide::Observed,
                ..
            })
        ));
    }

    #[test]
    fn matches_slice_based_audit_on_bounded_fixtures() {
        use crate::public_dump_coverage::{audit_page_coverage, PageCensus};
        let expected = [1, 3, 5, 7, 9];
        let observed = [1, 2, 7, 10];
        let slice = audit_page_coverage(
            PageCensus {
                project: scope().project,
                generation: scope().generation,
                namespace: 0,
                page_ids: &expected,
            },
            PageCensus {
                project: scope().project,
                generation: scope().generation,
                namespace: 0,
                page_ids: &observed,
            },
        );
        let stream = audit_page_coverage_stream(scope(), ids(&expected), scope(), ids(&observed));
        let Err(PageCoverageError::MissingOrUnexpected(a)) = slice else {
            panic!("slice audit should reject mismatch");
        };
        let Err(StreamCoverageError::MissingOrUnexpected(b)) = stream else {
            panic!("stream audit should reject mismatch");
        };
        assert_eq!(a, b);
    }
}
