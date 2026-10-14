//! Fail-closed bridge between validated pinned dump events and census export.
//!
//! A caller must still authenticate and verify compressed source members, decode
//! well-formed XML, and stage the output privately before atomic publication.
//! This bridge cannot supply an *independent* expected page census.

use std::io::Write;

use crate::public_dump_census_export::{CensusExportError, ObservedCensusWriter};
use crate::public_dump_ingest::{PageIngestError, PinnedPageIngest};
use crate::public_dump_page_meta::PageEvent;
use crate::public_dump_pinned::PinnedInventory;

#[derive(Debug)]
pub enum PinnedCensusError {
    InvalidState,
    Ingest(PageIngestError),
    Export(CensusExportError),
    CountMismatch,
}

/// The census bytes are returned only after all pinned members and decoded
/// page revisions pass their validation and the output stream flush succeeds.
/// Any unsuccessful input operation permanently poisons the pipeline.
pub struct PinnedObservedCensus<'a, W: Write> {
    ingest: PinnedPageIngest<'a>,
    writer: ObservedCensusWriter<W>,
    observed_pages: usize,
    failed: bool,
}

impl<'a, W: Write> PinnedObservedCensus<'a, W> {
    pub fn new(
        inventory: &'a PinnedInventory,
        output: W,
        namespace: i32,
    ) -> Result<Self, PinnedCensusError> {
        Ok(Self {
            ingest: PinnedPageIngest::new(inventory),
            writer: ObservedCensusWriter::new(output, namespace)
                .map_err(PinnedCensusError::Export)?,
            observed_pages: 0,
            failed: false,
        })
    }

    pub fn begin_member(&mut self, relative_path: &str) -> Result<(), PinnedCensusError> {
        if self.failed {
            return Err(PinnedCensusError::InvalidState);
        }
        self.ingest.begin_member(relative_path).map_err(|error| {
            self.failed = true;
            PinnedCensusError::Ingest(error)
        })
    }

    pub fn push(&mut self, event: PageEvent<'_>) -> Result<(), PinnedCensusError> {
        if self.failed {
            return Err(PinnedCensusError::InvalidState);
        }
        let page = self.ingest.push(event).map_err(|error| {
            self.failed = true;
            PinnedCensusError::Ingest(error)
        })?;
        if let Some(page) = page {
            self.writer.push(&page).map_err(|error| {
                self.failed = true;
                PinnedCensusError::Export(error)
            })?;
            self.observed_pages = self
                .observed_pages
                .checked_add(1)
                .ok_or_else(|| {
                    self.failed = true;
                    PinnedCensusError::CountMismatch
                })?;
        }
        Ok(())
    }

    pub fn end_member(&mut self) -> Result<(), PinnedCensusError> {
        if self.failed {
            return Err(PinnedCensusError::InvalidState);
        }
        self.ingest.end_member().map_err(|error| {
            self.failed = true;
            PinnedCensusError::Ingest(error)
        })
    }

    pub fn finish(self) -> Result<(W, usize), PinnedCensusError> {
        if self.failed {
            return Err(PinnedCensusError::InvalidState);
        }
        let audited = self.ingest.finish().map_err(PinnedCensusError::Ingest)?;
        if audited != self.observed_pages {
            return Err(PinnedCensusError::CountMismatch);
        }
        self.writer.finish().map_err(PinnedCensusError::Export)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use sha2::{Digest, Sha256};
    use std::io;

    fn inventory() -> PinnedInventory {
        let sums = format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n{}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap()
    }

    fn push_page<W: Write>(
        pipeline: &mut PinnedObservedCensus<'_, W>,
        page_id: u64,
        namespace: i32,
    ) -> Result<(), PinnedCensusError> {
        pipeline.push(PageEvent::BeginPage)?;
        pipeline.push(PageEvent::Title("Validated page"))?;
        pipeline.push(PageEvent::Namespace(namespace))?;
        pipeline.push(PageEvent::PageId(page_id))?;
        pipeline.push(PageEvent::BeginRevision)?;
        pipeline.push(PageEvent::RevisionId(page_id + 1000))?;
        pipeline.push(PageEvent::Timestamp("2026-10-13T12:00:00Z"))?;
        pipeline.push(PageEvent::EndRevision)?;
        pipeline.push(PageEvent::EndPage)
    }

    #[test]
    fn validated_multi_shard_pages_produce_only_selected_namespace_ids() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        pipeline.begin_member(&pinned.members()[0].relative_path).unwrap();
        push_page(&mut pipeline, 1, 0).unwrap();
        push_page(&mut pipeline, 2, 1).unwrap();
        pipeline.end_member().unwrap();
        pipeline.begin_member(&pinned.members()[1].relative_path).unwrap();
        push_page(&mut pipeline, 201, 0).unwrap();
        pipeline.end_member().unwrap();
        let (bytes, namespace_count) = pipeline.finish().unwrap();
        assert_eq!(bytes, b"1\n201\n");
        assert_eq!(namespace_count, 2);
    }

    #[test]
    fn unfinished_shards_and_invalid_pages_never_return_census_bytes() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        pipeline.begin_member(&pinned.members()[0].relative_path).unwrap();
        push_page(&mut pipeline, 1, 0).unwrap();
        pipeline.end_member().unwrap();
        assert!(matches!(pipeline.finish(), Err(PinnedCensusError::Ingest(_))));

        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        pipeline.begin_member(&pinned.members()[0].relative_path).unwrap();
        push_page(&mut pipeline, 1, 0).unwrap();
        assert!(matches!(
            push_page(&mut pipeline, 1, 0),
            Err(PinnedCensusError::Ingest(_))
        ));
        assert!(matches!(pipeline.end_member(), Err(PinnedCensusError::InvalidState)));
        assert!(matches!(pipeline.finish(), Err(PinnedCensusError::InvalidState)));
    }

    struct RejectingWriter;

    impl Write for RejectingWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed staging sink"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn staging_write_failure_poisons_entire_pinned_ingestion() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, RejectingWriter, 0).unwrap();
        pipeline.begin_member(&pinned.members()[0].relative_path).unwrap();
        assert!(matches!(
            push_page(&mut pipeline, 1, 0),
            Err(PinnedCensusError::Export(CensusExportError::Write(_)))
        ));
        assert!(matches!(pipeline.end_member(), Err(PinnedCensusError::InvalidState)));
        assert!(matches!(pipeline.finish(), Err(PinnedCensusError::InvalidState)));
    }
}
