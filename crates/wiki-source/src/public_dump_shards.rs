//! Exact pinned-inventory member correspondence for decoded XML page IDs.
//! This validates shard membership and ranges, not publisher authenticity,
//! missing pages within ranges, or revision/rendering fidelity.

use crate::public_dump_coverage::{audit_shard_page_ids, ShardPageError};
use crate::public_dump_pinned::PinnedInventory;

#[derive(Clone, Copy, Debug)]
pub struct ObservedShard<'a> {
    pub relative_path: &'a str,
    pub page_ids: &'a [u64],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportShardError {
    CountMismatch { expected: usize, observed: usize },
    MemberMismatch { index: usize },
    InvalidShard { index: usize, error: ShardPageError },
    TooManyPages,
}

/// Require exactly one decoded member per pinned checksum entry, in inventory
/// order. The caller must independently verify the source and decode XML.
pub fn audit_export_shards(
    inventory: &PinnedInventory,
    observed: &[ObservedShard<'_>],
) -> Result<usize, ExportShardError> {
    let members = inventory.members();
    if members.len() != observed.len() {
        return Err(ExportShardError::CountMismatch {
            expected: members.len(),
            observed: observed.len(),
        });
    }
    let mut total = 0usize;
    for (index, (member, shard)) in members.iter().zip(observed).enumerate() {
        if member.relative_path != shard.relative_path {
            return Err(ExportShardError::MemberMismatch { index });
        }
        let count = audit_shard_page_ids(
            member,
            inventory.project(),
            inventory.date(),
            shard.page_ids,
        )
        .map_err(|error| ExportShardError::InvalidShard { index, error })?;
        total = total
            .checked_add(count)
            .ok_or(ExportShardError::TooManyPages)?;
        if total > 100_000_000 {
            return Err(ExportShardError::TooManyPages);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use sha2::{Digest, Sha256};

    #[test]
    fn requires_exact_member_order_and_valid_page_partitions() {
        let raw = format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n{}  enwiki-2026-10-01-p101p200.xml.bz2\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let inventory =
            parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let first = inventory.members()[0].relative_path.as_str();
        let second = inventory.members()[1].relative_path.as_str();
        let valid = [
            ObservedShard {
                relative_path: first,
                page_ids: &[101, 200],
            },
            ObservedShard {
                relative_path: second,
                page_ids: &[1, 50, 100],
            },
        ];
        assert_eq!(audit_export_shards(&inventory, &valid), Ok(5));
        assert_eq!(
            audit_export_shards(&inventory, &valid[..1]),
            Err(ExportShardError::CountMismatch {
                expected: 2,
                observed: 1
            })
        );
        assert_eq!(
            audit_export_shards(&inventory, &[valid[1], valid[0]]),
            Err(ExportShardError::MemberMismatch { index: 0 })
        );
        let invalid = [
            ObservedShard {
                relative_path: first,
                page_ids: &[101, 201],
            },
            valid[1],
        ];
        assert_eq!(
            audit_export_shards(&inventory, &invalid),
            Err(ExportShardError::InvalidShard {
                index: 0,
                error: ShardPageError::OutsideDeclaredRange {
                    page_id: 201,
                    first: 101,
                    last: 200,
                },
            })
        );
    }
}
