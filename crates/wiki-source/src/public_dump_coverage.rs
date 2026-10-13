//! Fail-closed, constant-memory comparison of two sorted page-ID censuses.
//!
//! A valid SHA256SUMS and intact members do not establish page coverage:
//! an upstream export may omit pages while retaining valid file checksums.
//! The expected census MUST come from a separately qualified source with
//! equivalent snapshot cutoffs and namespace filters. Equal caller-supplied
//! labels are not authentication or independent evidence of completeness.

/// A strictly sorted census of article identities for one namespace.
#[derive(Clone, Copy, Debug)]
pub struct PageCensus<'a> {
    pub project: &'a str,
    pub generation: &'a str,
    pub namespace: i32,
    pub page_ids: &'a [u64],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageCoverageError {
    IncompatibleCensus,
    InvalidCensus,
    UnsortedOrDuplicate,
    TooManyPages,
    MissingOrUnexpected(PageCoverageDelta),
}

/// Retain bounded diagnostic examples without cloning millions of page IDs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageCoverageDelta {
    pub expected_count: usize,
    pub observed_count: usize,
    pub missing_count: usize,
    pub unexpected_count: usize,
    pub missing_examples: Vec<u64>,
    pub unexpected_examples: Vec<u64>,
}

const MAX_IDS: usize = 100_000_000;
const MAX_EXAMPLES: usize = 12;

fn validate(census: PageCensus<'_>) -> Result<(), PageCoverageError> {
    if census.project.is_empty() || census.generation.is_empty() || census.page_ids.is_empty() {
        return Err(PageCoverageError::InvalidCensus);
    }
    if census.page_ids.len() > MAX_IDS {
        return Err(PageCoverageError::TooManyPages);
    }
    let mut previous = 0;
    for &id in census.page_ids {
        if id <= previous {
            return Err(PageCoverageError::UnsortedOrDuplicate);
        }
        previous = id;
    }
    Ok(())
}

/// Compare independently prepared page-ID inventories for the SAME snapshot.
/// This is a diagnostic guard and MUST NOT alone certify a complete publisher
/// release or activate an offline pack. IDs must be positive, strictly sorted,
/// unique, and scoped to the same namespace, project and generation.
pub fn audit_page_coverage(
    expected: PageCensus<'_>,
    observed: PageCensus<'_>,
) -> Result<PageCoverageDelta, PageCoverageError> {
    if expected.project != observed.project
        || expected.generation != observed.generation
        || expected.namespace != observed.namespace
    {
        return Err(PageCoverageError::IncompatibleCensus);
    }
    validate(expected)?;
    validate(observed)?;

    let mut delta = PageCoverageDelta {
        expected_count: expected.page_ids.len(),
        observed_count: observed.page_ids.len(),
        missing_count: 0,
        unexpected_count: 0,
        missing_examples: Vec::new(),
        unexpected_examples: Vec::new(),
    };
    let mut a = 0;
    let mut b = 0;
    while a < expected.page_ids.len() && b < observed.page_ids.len() {
        let wanted = expected.page_ids[a];
        let found = observed.page_ids[b];
        match wanted.cmp(&found) {
            std::cmp::Ordering::Less => {
                delta.missing_count += 1;
                if delta.missing_examples.len() < MAX_EXAMPLES {
                    delta.missing_examples.push(wanted);
                }
                a += 1;
            }
            std::cmp::Ordering::Greater => {
                delta.unexpected_count += 1;
                if delta.unexpected_examples.len() < MAX_EXAMPLES {
                    delta.unexpected_examples.push(found);
                }
                b += 1;
            }
            std::cmp::Ordering::Equal => {
                a += 1;
                b += 1;
            }
        }
    }
    while a < expected.page_ids.len() {
        delta.missing_count += 1;
        if delta.missing_examples.len() < MAX_EXAMPLES {
            delta.missing_examples.push(expected.page_ids[a]);
        }
        a += 1;
    }
    while b < observed.page_ids.len() {
        delta.unexpected_count += 1;
        if delta.unexpected_examples.len() < MAX_EXAMPLES {
            delta.unexpected_examples.push(observed.page_ids[b]);
        }
        b += 1;
    }
    if delta.missing_count != 0 || delta.unexpected_count != 0 {
        Err(PageCoverageError::MissingOrUnexpected(delta))
    } else {
        Ok(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn census(ids: &[u64]) -> PageCensus<'_> {
        PageCensus {
            project: "enwiki",
            generation: "2026-07-01",
            namespace: 0,
            page_ids: ids,
        }
    }

    #[test]
    fn identical_census_passes_without_implying_publisher_authenticity() {
        let delta = audit_page_coverage(census(&[1, 2, 4]), census(&[1, 2, 4])).unwrap();
        assert_eq!(delta.expected_count, 3);
        assert_eq!(delta.missing_count, 0);
        assert_eq!(delta.unexpected_count, 0);
    }

    #[test]
    fn checksum_valid_but_missing_pages_fails_closed() {
        let result =
            audit_page_coverage(census(&[100, 101, 102, 103, 104]), census(&[100, 102, 104]));
        assert_eq!(
            result,
            Err(PageCoverageError::MissingOrUnexpected(PageCoverageDelta {
                expected_count: 5,
                observed_count: 3,
                missing_count: 2,
                unexpected_count: 0,
                missing_examples: vec![101, 103],
                unexpected_examples: vec![],
            }))
        );
    }

    #[test]
    fn detects_unexpected_and_missing_ids_and_bounds_diagnostics() {
        let expected: Vec<_> = (1..=50).collect();
        let actual = [2, 4, 1000, 2000];
        let result = audit_page_coverage(census(&expected), census(&actual));
        let Err(PageCoverageError::MissingOrUnexpected(delta)) = result else {
            panic!("expected coverage failure");
        };
        assert_eq!(delta.missing_count, 48);
        assert_eq!(delta.unexpected_count, 2);
        assert_eq!(delta.missing_examples.len(), MAX_EXAMPLES);
        assert_eq!(delta.unexpected_examples, vec![1000, 2000]);
    }

    #[test]
    fn rejects_noncomparable_empty_duplicate_unsorted_or_zero_id_inputs() {
        let ids = [1, 2, 3];
        assert_eq!(
            audit_page_coverage(
                census(&ids),
                PageCensus {
                    namespace: 1,
                    ..census(&ids)
                }
            ),
            Err(PageCoverageError::IncompatibleCensus)
        );
        assert_eq!(
            audit_page_coverage(
                census(&ids),
                PageCensus {
                    generation: "2026-08-01",
                    ..census(&ids)
                }
            ),
            Err(PageCoverageError::IncompatibleCensus)
        );
        assert_eq!(
            audit_page_coverage(census(&ids), census(&[])),
            Err(PageCoverageError::InvalidCensus)
        );
        for corrupt in [&[0, 1][..], &[3, 2][..], &[1, 1][..]] {
            assert_eq!(
                audit_page_coverage(census(&ids), census(corrupt)),
                Err(PageCoverageError::UnsortedOrDuplicate)
            );
        }
    }
}
