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
    if !crate::valid_wiki_project(census.project)
        || census.namespace < 0
        || census.generation.is_empty()
        || census.generation.len() > 255
        || census.generation.starts_with('.')
        || census.generation.ends_with('.')
        || !census
            .generation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        || census.page_ids.is_empty()
    {
        return Err(PageCoverageError::InvalidCensus);
    }
    if census.page_ids.len() > MAX_IDS {
        return Err(PageCoverageError::TooManyPages);
    }
    let mut previous = 0;
    for &id in census.page_ids {
        if id <= previous || id > i64::MAX as u64 {
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

/// One shard's page IDs must be sorted and within its checksum-declared
/// inclusive range. A range match does not establish page completeness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShardPageError {
    InvalidMember,
    EmptyShard,
    TooManyPages,
    UnsortedOrDuplicate,
    OutsideDeclaredRange { page_id: u64, first: u64, last: u64 },
}

/// Check page identities extracted from one verified current-content XML shard.
/// A valid member filename and sorted page IDs are necessary, NOT sufficient,
/// to establish snapshot completeness, exact revisions or media fidelity.
pub fn audit_shard_page_ids(
    member: &crate::public_dump::DumpChecksum,
    wiki: &str,
    date: &str,
    page_ids: &[u64],
) -> Result<usize, ShardPageError> {
    let (first, last) = crate::public_dump::declared_member_page_range(member, wiki, date)
        .ok_or(ShardPageError::InvalidMember)?;
    if page_ids.is_empty() {
        return Err(ShardPageError::EmptyShard);
    }
    if page_ids.len() > MAX_IDS {
        return Err(ShardPageError::TooManyPages);
    }
    let mut previous = 0;
    for &id in page_ids {
        if id <= previous || id > i64::MAX as u64 {
            return Err(ShardPageError::UnsortedOrDuplicate);
        }
        if id < first || id > last {
            return Err(ShardPageError::OutsideDeclaredRange {
                page_id: id,
                first,
                last,
            });
        }
        previous = id;
    }
    Ok(page_ids.len())
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

    #[test]
    fn census_identity_must_not_accept_unsafe_project_or_generation_labels() {
        let ids = [1, 2, 3];
        for project in [
            "Enwiki", "en/wiki", "../wiki", "a b", "", "_enwiki", "enwiki_", "123wiki",
        ] {
            let invalid = PageCensus {
                project,
                ..census(&ids)
            };
            assert_eq!(
                audit_page_coverage(invalid, invalid),
                Err(PageCoverageError::InvalidCensus)
            );
        }
        let invalid = PageCensus {
            namespace: -1,
            ..census(&ids)
        };
        assert_eq!(
            audit_page_coverage(invalid, invalid),
            Err(PageCoverageError::InvalidCensus)
        );
        for generation in [".hidden", "2026/10/01", "../outside", "bad label", ""] {
            let invalid = PageCensus {
                generation,
                ..census(&ids)
            };
            assert_eq!(
                audit_page_coverage(invalid, invalid),
                Err(PageCoverageError::InvalidCensus)
            );
        }
    }

    #[test]
    fn verified_shard_ids_must_respect_declared_partition() {
        use crate::public_dump::parse_sha256sums;
        let member = parse_sha256sums(&format!(
            "{}  enwiki-2026-10-01-p101p200.xml.bz2",
            "a".repeat(64)
        ))
        .unwrap()
        .remove(0);
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[101, 140, 200]),
            Ok(3)
        );
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[100, 140]),
            Err(ShardPageError::OutsideDeclaredRange {
                page_id: 100,
                first: 101,
                last: 200,
            })
        );
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[101, 201]),
            Err(ShardPageError::OutsideDeclaredRange {
                page_id: 201,
                first: 101,
                last: 200,
            })
        );
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[102, 102]),
            Err(ShardPageError::UnsortedOrDuplicate)
        );
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[]),
            Err(ShardPageError::EmptyShard)
        );
        assert_eq!(
            audit_shard_page_ids(&member, "frwiki", "2026-10-01", &[101]),
            Err(ShardPageError::InvalidMember)
        );
    }

    #[test]
    fn rejects_page_ids_outside_signed_catalog_range() {
        let too_large = i64::MAX as u64 + 1;
        assert_eq!(
            audit_page_coverage(census(&[1]), census(&[too_large])),
            Err(PageCoverageError::UnsortedOrDuplicate)
        );
        use crate::public_dump::parse_sha256sums;
        let member = parse_sha256sums(&format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2",
            "a".repeat(64)
        ))
        .unwrap()
        .remove(0);
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[too_large]),
            Err(ShardPageError::UnsortedOrDuplicate)
        );
    }

    #[test]
    fn legacy_numbered_shards_obey_the_same_partition_rule() {
        use crate::public_dump::parse_sha256sums;
        let member = parse_sha256sums(&format!(
            "{}  enwiki-20261001-pages-meta-current2.xml-p101p200.bz2",
            "a".repeat(64)
        ))
        .unwrap()
        .remove(0);
        assert_eq!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[101, 200]),
            Ok(2)
        );
        assert!(matches!(
            audit_shard_page_ids(&member, "enwiki", "2026-10-01", &[201]),
            Err(ShardPageError::OutsideDeclaredRange { .. })
        ));
    }
}
