//! Constant-memory page-ID audit for a sequential public-dump XML decoder.
//!
//! The decoder must consume only independently verified staged members and
//! emit the outer `<page><id>` identity (not revision or contributor IDs).
//! This audit does not parse XML, authenticate the source, or prove coverage
//! against an independently obtained page census.

use crate::public_dump::declared_member_page_range;
use crate::public_dump_pinned::PinnedInventory;

const MAX_PAGES: usize = 100_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamAuditError {
    InvalidState,
    MissingMember {
        index: usize,
    },
    MemberMismatch {
        index: usize,
    },
    InvalidRange {
        index: usize,
    },
    EmptyMember {
        index: usize,
    },
    UnsortedOrDuplicate {
        index: usize,
        page_id: u64,
    },
    OutsideRange {
        index: usize,
        page_id: u64,
        first: u64,
        last: u64,
    },
    TooManyPages,
}

/// A single-pass, O(1)-memory counterpart to `audit_export_shards`.
/// Every declared member must be visited exactly once in inventory order.
pub struct StreamShardAudit<'a> {
    inventory: &'a PinnedInventory,
    next_member: usize,
    active: bool,
    failed: bool,
    first: u64,
    last: u64,
    previous_id: u64,
    member_pages: usize,
    total_pages: usize,
}

impl<'a> StreamShardAudit<'a> {
    pub fn new(inventory: &'a PinnedInventory) -> Self {
        Self {
            inventory,
            next_member: 0,
            active: false,
            failed: false,
            first: 0,
            last: 0,
            previous_id: 0,
            member_pages: 0,
            total_pages: 0,
        }
    }

    /// Check the complete descriptor, including SHA-256, against the next
    /// inventory member before a caller-controlled decoder is allowed to run.
    pub(crate) fn matches_next_member(&self, member: &crate::public_dump::DumpChecksum) -> bool {
        !self.failed
            && !self.active
            && self
                .inventory
                .members()
                .get(self.next_member)
                .is_some_and(|expected| expected == member)
    }

    /// Start one already-verified member. The caller owns decoding and I/O.
    /// A failed operation permanently invalidates this audit instance.
    pub fn begin_member(&mut self, relative_path: &str) -> Result<(), StreamAuditError> {
        if self.failed || self.active {
            self.failed = true;
            return Err(StreamAuditError::InvalidState);
        }
        let index = self.next_member;
        let member = match self.inventory.members().get(index) {
            Some(member) => member,
            None => {
                self.failed = true;
                return Err(StreamAuditError::MemberMismatch { index });
            }
        };
        if member.relative_path != relative_path {
            self.failed = true;
            return Err(StreamAuditError::MemberMismatch { index });
        }
        let (first, last) = match declared_member_page_range(
            member,
            self.inventory.project(),
            self.inventory.date(),
        ) {
            Some(range) => range,
            None => {
                self.failed = true;
                return Err(StreamAuditError::InvalidRange { index });
            }
        };
        self.first = first;
        self.last = last;
        self.previous_id = 0;
        self.member_pages = 0;
        self.active = true;
        Ok(())
    }

    /// Observe one outer page identity in decoded XML document order.
    /// A rejected page must never be ignored by a caller that later finishes.
    pub fn push_page_id(&mut self, page_id: u64) -> Result<(), StreamAuditError> {
        if self.failed || !self.active {
            self.failed = true;
            return Err(StreamAuditError::InvalidState);
        }
        let index = self.next_member;
        if page_id <= self.previous_id {
            self.failed = true;
            return Err(StreamAuditError::UnsortedOrDuplicate { index, page_id });
        }
        if page_id < self.first || page_id > self.last {
            self.failed = true;
            return Err(StreamAuditError::OutsideRange {
                index,
                page_id,
                first: self.first,
                last: self.last,
            });
        }
        if self.total_pages >= MAX_PAGES {
            self.failed = true;
            return Err(StreamAuditError::TooManyPages);
        }
        self.member_pages += 1;
        self.total_pages += 1;
        self.previous_id = page_id;
        Ok(())
    }

    /// Reject a shard that decoded no outer page records.
    pub fn end_member(&mut self) -> Result<(), StreamAuditError> {
        if self.failed || !self.active {
            self.failed = true;
            return Err(StreamAuditError::InvalidState);
        }
        if self.member_pages == 0 {
            self.failed = true;
            return Err(StreamAuditError::EmptyMember {
                index: self.next_member,
            });
        }
        self.active = false;
        self.next_member += 1;
        Ok(())
    }

    /// Require all pinned members to have been fully consumed without ANY
    /// preceding validation error, even if the caller discarded that error.
    pub fn finish(self) -> Result<usize, StreamAuditError> {
        if self.failed || self.active {
            return Err(StreamAuditError::InvalidState);
        }
        if self.next_member != self.inventory.members().len() {
            return Err(StreamAuditError::MissingMember {
                index: self.next_member,
            });
        }
        Ok(self.total_pages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use sha2::{Digest, Sha256};

    fn fixture() -> PinnedInventory {
        let raw = format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n{}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap()
    }

    #[test]
    fn audits_complete_ordered_shards_without_collecting_ids() {
        let inventory = fixture();
        let mut audit = StreamShardAudit::new(&inventory);
        for member in inventory.members() {
            audit.begin_member(&member.relative_path).unwrap();
            if member.relative_path.contains("p201p300") {
                audit.push_page_id(201).unwrap();
                audit.push_page_id(300).unwrap();
            } else {
                audit.push_page_id(1).unwrap();
                audit.push_page_id(100).unwrap();
            }
            audit.end_member().unwrap();
        }
        assert_eq!(audit.finish(), Ok(4));
    }

    #[test]
    fn rejects_missing_reordered_and_unfinished_members() {
        let inventory = fixture();
        let first = &inventory.members()[0].relative_path;
        let second = &inventory.members()[1].relative_path;

        let mut audit = StreamShardAudit::new(&inventory);
        assert_eq!(audit.push_page_id(1), Err(StreamAuditError::InvalidState));
        assert_eq!(
            audit.begin_member(first),
            Err(StreamAuditError::InvalidState)
        );
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));

        let mut audit = StreamShardAudit::new(&inventory);
        assert_eq!(
            audit.begin_member(second),
            Err(StreamAuditError::MemberMismatch { index: 0 })
        );
        assert_eq!(
            audit.begin_member(first),
            Err(StreamAuditError::InvalidState)
        );
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));

        let mut audit = StreamShardAudit::new(&inventory);
        audit.begin_member(first).unwrap();
        assert_eq!(
            audit.begin_member(first),
            Err(StreamAuditError::InvalidState)
        );
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));

        let mut audit = StreamShardAudit::new(&inventory);
        audit.begin_member(first).unwrap();
        assert_eq!(
            audit.end_member(),
            Err(StreamAuditError::EmptyMember { index: 0 })
        );
        assert_eq!(audit.push_page_id(1), Err(StreamAuditError::InvalidState));
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));

        let mut audit = StreamShardAudit::new(&inventory);
        audit.begin_member(first).unwrap();
        audit.push_page_id(1).unwrap();
        audit.end_member().unwrap();
        assert_eq!(
            audit.finish(),
            Err(StreamAuditError::MissingMember { index: 1 })
        );

        let mut audit = StreamShardAudit::new(&inventory);
        audit.begin_member(first).unwrap();
        audit.push_page_id(1).unwrap();
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));
    }

    #[test]
    fn invalid_page_permanently_poisoned_even_if_caller_ignores_error() {
        let inventory = fixture();
        let first = &inventory.members()[0].relative_path;
        let cases = [
            (
                10,
                10,
                StreamAuditError::UnsortedOrDuplicate {
                    index: 0,
                    page_id: 10,
                },
            ),
            (
                10,
                9,
                StreamAuditError::UnsortedOrDuplicate {
                    index: 0,
                    page_id: 9,
                },
            ),
            (
                10,
                101,
                StreamAuditError::OutsideRange {
                    index: 0,
                    page_id: 101,
                    first: 1,
                    last: 100,
                },
            ),
            (
                10,
                0,
                StreamAuditError::UnsortedOrDuplicate {
                    index: 0,
                    page_id: 0,
                },
            ),
        ];
        for (first_page, bad_page, expected) in cases {
            let mut audit = StreamShardAudit::new(&inventory);
            audit.begin_member(first).unwrap();
            audit.push_page_id(first_page).unwrap();
            assert_eq!(audit.push_page_id(bad_page), Err(expected));
            assert_eq!(audit.push_page_id(100), Err(StreamAuditError::InvalidState));
            assert_eq!(audit.end_member(), Err(StreamAuditError::InvalidState));
            assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));
        }
    }

    #[test]
    fn duplicate_after_valid_first_shard_cannot_be_laundered_by_ending_second() {
        let inventory = fixture();
        let mut audit = StreamShardAudit::new(&inventory);
        audit
            .begin_member(&inventory.members()[0].relative_path)
            .unwrap();
        audit.push_page_id(1).unwrap();
        audit.end_member().unwrap();
        audit
            .begin_member(&inventory.members()[1].relative_path)
            .unwrap();
        audit.push_page_id(201).unwrap();
        assert_eq!(
            audit.push_page_id(201),
            Err(StreamAuditError::UnsortedOrDuplicate {
                index: 1,
                page_id: 201,
            })
        );
        assert_eq!(audit.end_member(), Err(StreamAuditError::InvalidState));
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));
    }
}
