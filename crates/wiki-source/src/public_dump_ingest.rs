//! Connect validated public-dump page metadata to the pinned shard audit.
//!
//! The caller MUST supply semantic events from a well-formed, trusted XML
//! decoder after independently verifying each compressed member's digest.
//! This adapter does not tokenize XML, decompress bzip2, render wikitext,
//! authenticate upstream publications, or establish page-census completeness.

use crate::public_dump_page_meta::{
    PageEvent, PageMetadataCollector, PageMetadataError, PublicDumpPage,
};
use crate::public_dump_pinned::PinnedInventory;
use crate::public_dump_stream::{StreamAuditError, StreamShardAudit};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageIngestError {
    InvalidState,
    IncompletePage,
    Metadata(PageMetadataError),
    Shard(StreamAuditError),
}

/// A fail-closed event sink for one pinned, ordered multi-shard export.
///
/// No page is accepted until its metadata is complete and its outer page ID
/// passes the matching shard's range, uniqueness and ordering constraints.
/// Errors permanently poison the sink, even when the caller ignores them.
pub struct PinnedPageIngest<'a> {
    audit: StreamShardAudit<'a>,
    metadata: PageMetadataCollector,
    active: bool,
    in_page: bool,
    failed: bool,
}

impl<'a> PinnedPageIngest<'a> {
    pub fn new(inventory: &'a PinnedInventory) -> Self {
        Self {
            audit: StreamShardAudit::new(inventory),
            metadata: PageMetadataCollector::new(),
            active: false,
            in_page: false,
            failed: false,
        }
    }

    pub fn begin_member(&mut self, relative_path: &str) -> Result<(), PageIngestError> {
        if self.failed || self.active || self.in_page {
            self.failed = true;
            return Err(PageIngestError::InvalidState);
        }
        if let Err(error) = self.audit.begin_member(relative_path) {
            self.failed = true;
            return Err(PageIngestError::Shard(error));
        }
        self.active = true;
        Ok(())
    }

    pub fn push(
        &mut self,
        event: PageEvent<'_>,
    ) -> Result<Option<PublicDumpPage>, PageIngestError> {
        if self.failed || !self.active {
            self.failed = true;
            return Err(PageIngestError::InvalidState);
        }
        let is_begin = matches!(event, PageEvent::BeginPage);
        let is_end = matches!(event, PageEvent::EndPage);
        let page = match self.metadata.push(event) {
            Ok(page) => page,
            Err(error) => {
                self.failed = true;
                return Err(PageIngestError::Metadata(error));
            }
        };
        if is_begin {
            self.in_page = true;
        }
        if is_end {
            self.in_page = false;
        }
        if let Some(ref complete) = page {
            if let Err(error) = self.audit.push_page_id(complete.page_id) {
                self.failed = true;
                return Err(PageIngestError::Shard(error));
            }
        }
        Ok(page)
    }

    pub fn end_member(&mut self) -> Result<(), PageIngestError> {
        if self.failed || !self.active {
            self.failed = true;
            return Err(PageIngestError::InvalidState);
        }
        if self.in_page {
            self.failed = true;
            return Err(PageIngestError::IncompletePage);
        }
        if let Err(error) = self.audit.end_member() {
            self.failed = true;
            return Err(PageIngestError::Shard(error));
        }
        self.active = false;
        Ok(())
    }

    /// Only a complete sequence of fully decoded members may finish.
    /// The result counts accepted pages, not independently verified coverage.
    pub fn finish(self) -> Result<usize, PageIngestError> {
        if self.failed || self.active || self.in_page {
            return Err(PageIngestError::InvalidState);
        }
        self.metadata.finish().map_err(PageIngestError::Metadata)?;
        self.audit.finish().map_err(PageIngestError::Shard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use sha2::{Digest, Sha256};

    fn inventory() -> PinnedInventory {
        let sums = format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n{}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap()
    }

    fn page(ingest: &mut PinnedPageIngest<'_>, id: u64) -> Result<PublicDumpPage, PageIngestError> {
        ingest.push(PageEvent::BeginPage)?;
        ingest.push(PageEvent::Title("München"))?;
        ingest.push(PageEvent::Namespace(0))?;
        ingest.push(PageEvent::PageId(id))?;
        ingest.push(PageEvent::BeginRevision)?;
        ingest.push(PageEvent::RevisionId(id + 1000))?;
        ingest.push(PageEvent::Timestamp("2026-10-13T12:00:00Z"))?;
        ingest.push(PageEvent::EndRevision)?;
        Ok(ingest.push(PageEvent::EndPage)?.unwrap())
    }

    #[test]
    fn binds_complete_revision_metadata_to_pinned_shard_order() {
        let pinned = inventory();
        let mut ingest = PinnedPageIngest::new(&pinned);
        ingest
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        assert_eq!(page(&mut ingest, 1).unwrap().revision_id, 1001);
        assert_eq!(page(&mut ingest, 100).unwrap().title, "München");
        ingest.end_member().unwrap();
        ingest
            .begin_member(&pinned.members()[1].relative_path)
            .unwrap();
        assert_eq!(page(&mut ingest, 201).unwrap().page_id, 201);
        ingest.end_member().unwrap();
        assert_eq!(ingest.finish(), Ok(3));
    }

    #[test]
    fn rejects_out_of_range_and_duplicate_ids_without_recovery() {
        let pinned = inventory();
        for ids in [vec![101], vec![20, 20], vec![20, 19]] {
            let mut ingest = PinnedPageIngest::new(&pinned);
            ingest
                .begin_member(&pinned.members()[0].relative_path)
                .unwrap();
            for id in ids {
                if page(&mut ingest, id).is_err() {
                    break;
                }
            }
            assert_eq!(ingest.end_member(), Err(PageIngestError::InvalidState));
            assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));
        }
    }

    #[test]
    fn cannot_finish_with_open_page_or_missing_shard() {
        let pinned = inventory();
        let mut ingest = PinnedPageIngest::new(&pinned);
        ingest
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        ingest.push(PageEvent::BeginPage).unwrap();
        assert_eq!(ingest.end_member(), Err(PageIngestError::IncompletePage));
        assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));

        let mut ingest = PinnedPageIngest::new(&pinned);
        ingest
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        page(&mut ingest, 1).unwrap();
        ingest.end_member().unwrap();
        assert_eq!(
            ingest.finish(),
            Err(PageIngestError::Shard(StreamAuditError::MissingMember {
                index: 1
            }))
        );
    }

    #[test]
    fn metadata_failure_poisoning_prevents_false_completion() {
        let pinned = inventory();
        let mut ingest = PinnedPageIngest::new(&pinned);
        ingest
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        ingest.push(PageEvent::BeginPage).unwrap();
        ingest.push(PageEvent::Title("Example")).unwrap();
        ingest.push(PageEvent::Namespace(0)).unwrap();
        ingest.push(PageEvent::PageId(1)).unwrap();
        assert_eq!(
            ingest.push(PageEvent::PageId(2)),
            Err(PageIngestError::Metadata(PageMetadataError::DuplicateField))
        );
        assert_eq!(
            ingest.push(PageEvent::BeginRevision),
            Err(PageIngestError::InvalidState)
        );
        assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));
    }
}
