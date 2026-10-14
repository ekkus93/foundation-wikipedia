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
            first: 0,
            last: 0,
            previous_id: 0,
            member_pages: 0,
            total_pages: 0,
        }
    }

    /// Start one already-verified member. The caller owns decoding and I/O.
    pub fn begin_member(&mut self, relative_path: &str) -> Result<(), StreamAuditError> {
        if self.active {
            return Err(StreamAuditError::InvalidState);
        }
        let index = self.next_member;
        let member = self
            .inventory
            .members()
            .get(index)
            .ok_or(StreamAuditError::MemberMismatch { index })?;
        if member.relative_path != relative_path {
            return Err(StreamAuditError::MemberMismatch { index });
        }
        let (first, last) =
            declared_member_page_range(member, self.inventory.project(), self.inventory.date())
                .ok_or(StreamAuditError::InvalidRange { index })?;
        self.first = first;
        self.last = last;
        self.previous_id = 0;
        self.member_pages = 0;
        self.active = true;
        Ok(())
    }

    /// Observe one outer page identity in decoded XML document order.
    pub fn push_page_id(&mut self, page_id: u64) -> Result<(), StreamAuditError> {
        if !self.active {
            return Err(StreamAuditError::InvalidState);
        }
        let index = self.next_member;
        if page_id <= self.previous_id {
            return Err(StreamAuditError::UnsortedOrDuplicate { index, page_id });
        }
        if page_id < self.first || page_id > self.last {
            return Err(StreamAuditError::OutsideRange {
                index,
                page_id,
                first: self.first,
                last: self.last,
            });
        }
        if self.total_pages >= MAX_PAGES {
            return Err(StreamAuditError::TooManyPages);
        }
        self.member_pages += 1;
        self.total_pages += 1;
        self.previous_id = page_id;
        Ok(())
    }

    /// Reject a shard that decoded no outer page records.
    pub fn end_member(&mut self) -> Result<(), StreamAuditError> {
        if !self.active {
            return Err(StreamAuditError::InvalidState);
        }
        if self.member_pages == 0 {
            return Err(StreamAuditError::EmptyMember {
                index: self.next_member,
            });
        }
        self.active = false;
        self.next_member += 1;
        Ok(())
    }

    /// Require all pinned members to have been fully consumed.
    pub fn finish(self) -> Result<usize, StreamAuditError> {
        if self.active {
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
        let mut audit = StreamShardAudit::new(&inventory);
        assert_eq!(audit.push_page_id(1), Err(StreamAuditError::InvalidState));
        assert_eq!(
            audit.begin_member(&inventory.members()[1].relative_path),
            Err(StreamAuditError::MemberMismatch { index: 0 })
        );
        audit
            .begin_member(&inventory.members()[0].relative_path)
            .unwrap();
        assert_eq!(
            audit.begin_member("anything"),
            Err(StreamAuditError::InvalidState)
        );
        assert_eq!(
            audit.end_member(),
            Err(StreamAuditError::EmptyMember { index: 0 })
        );
        audit.push_page_id(1).unwrap();
        audit.end_member().unwrap();
        assert_eq!(
            audit.finish(),
            Err(StreamAuditError::MissingMember { index: 1 })
        );

        let mut audit = StreamShardAudit::new(&inventory);
        audit
            .begin_member(&inventory.members()[0].relative_path)
            .unwrap();
        audit.push_page_id(1).unwrap();
        assert_eq!(audit.finish(), Err(StreamAuditError::InvalidState));
    }

    #[test]
    fn rejects_duplicate_unsorted_and_out_of_range_pages() {
        let inventory = fixture();
        let mut audit = StreamShardAudit::new(&inventory);
        audit
            .begin_member(&inventory.members()[0].relative_path)
            .unwrap();
        audit.push_page_id(10).unwrap();
        assert_eq!(
            audit.push_page_id(10),
            Err(StreamAuditError::UnsortedOrDuplicate {
                index: 0,
                page_id: 10
            })
        );
        assert_eq!(
            audit.push_page_id(9),
            Err(StreamAuditError::UnsortedOrDuplicate {
                index: 0,
                page_id: 9
            })
        );
        assert_eq!(
            audit.push_page_id(101),
            Err(StreamAuditError::OutsideRange {
                index: 0,
                page_id: 101,
                first: 1,
                last: 100
            })
        );
        audit.push_page_id(100).unwrap();
        audit.end_member().unwrap();
        audit
            .begin_member(&inventory.members()[1].relative_path)
            .unwrap();
        assert_eq!(
            audit.push_page_id(0),
            Err(StreamAuditError::UnsortedOrDuplicate {
                index: 1,
                page_id: 0
            })
        );
        audit.push_page_id(201).unwrap();
        audit.end_member().unwrap();
        assert_eq!(audit.finish(), Ok(3));
    }
}
