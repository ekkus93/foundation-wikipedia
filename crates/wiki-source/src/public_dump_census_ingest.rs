//! Fail-closed bridge between validated pinned dump events and census export.
//!
//! A caller must still authenticate and verify compressed source members, decode
//! well-formed XML, and stage the output privately before atomic publication.
//! This bridge cannot supply an *independent* expected page census.

use std::io::{Read, Write};

use crate::public_dump_census_export::{CensusExportError, ObservedCensusWriter};
use crate::public_dump_checked_read::CheckedMemberError;
use crate::public_dump_ingest::{PageIngestError, PinnedPageIngest};
use crate::public_dump_page_meta::PageEvent;
use crate::public_dump_pinned::PinnedInventory;

#[derive(Debug)]
pub enum PinnedCensusError {
    InvalidState,
    Ingest(PageIngestError),
    Export(CensusExportError),
    CountMismatch,
    Source(std::io::Error),
    NativeDecode(CheckedMemberError<std::io::Error>),
}

#[derive(Debug)]
pub enum NativeCensusError {
    Census(PinnedCensusError),
    Decode(CheckedMemberError<std::io::Error>),
}

#[derive(Debug)]
pub enum CheckedCensusError {
    Census(PinnedCensusError),
    Member(CheckedMemberError<PinnedCensusError>),
}

/// The census bytes are returned only after all pinned members and decoded
/// page revisions pass their validation and the output stream flush succeeds.
/// Any unsuccessful input operation permanently poisons the pipeline.
pub struct PinnedObservedCensus<'a, W: Write> {
    inventory: &'a PinnedInventory,
    next_member_index: usize,
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
            inventory,
            next_member_index: 0,
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

    /// Accept one pinned compressed member only after its decoder has consumed
    /// and SHA-256-verified the exact input bytes. A failed or prematurely
    /// returning decoder permanently poisons this census pipeline, even if it
    /// emitted otherwise valid page events before failure.
    ///
    /// The callback must independently verify bzip2/XML correctness and feed
    /// well-formed page events. It must not publish outputs or call end_member.
    /// This is a composition seam, not a bzip2 or XML decoder.
    pub fn decode_checked_member<R, F>(
        &mut self,
        input: R,
        max_bytes: u64,
        decoder: F,
    ) -> Result<(), CheckedCensusError>
    where
        R: Read,
        F: FnOnce(&mut dyn Read, &mut Self) -> Result<(), PinnedCensusError>,
    {
        // Obtain the complete trusted descriptor from the pinned inventory.
        // Accepting a caller-supplied checksum for the same filename would
        // allow a forged digest to authorize arbitrary compressed bytes.
        let Some(member) = self
            .inventory
            .members()
            .get(self.next_member_index)
            .cloned()
        else {
            self.failed = true;
            return Err(CheckedCensusError::Census(PinnedCensusError::InvalidState));
        };
        self.begin_member(&member.relative_path)
            .map_err(CheckedCensusError::Census)?;
        let result = crate::public_dump_checked_read::decode_checked_member(
            input,
            &member,
            max_bytes,
            |reader| decoder(reader, self),
        );
        if let Err(error) = result {
            self.failed = true;
            return Err(CheckedCensusError::Member(error));
        }
        self.end_member().map_err(CheckedCensusError::Census)
    }

    /// Decode the next inventory-pinned bzip2 shard using native Rust XML
    /// page events. Output stays in private staging until finish succeeds.
    ///
    /// The expected SHA-256 comes from the inventory, never the caller.
    /// On any decoder, metadata or checksum failure, the census is poisoned.
    pub fn decode_native_bzip2_member<R: Read>(
        &mut self,
        input: R,
        max_compressed_bytes: u64,
        max_decoded_bytes: u64,
    ) -> Result<(u64, u64), NativeCensusError> {
        if self.failed {
            return Err(NativeCensusError::Census(PinnedCensusError::InvalidState));
        }
        let Some(member) = self
            .inventory
            .members()
            .get(self.next_member_index)
            .cloned()
        else {
            self.failed = true;
            return Err(NativeCensusError::Census(PinnedCensusError::InvalidState));
        };
        self.begin_member(&member.relative_path)
            .map_err(NativeCensusError::Census)?;
        let mut callback_error = None;
        let result = crate::public_dump_bzip2::decode_bzip2_xml_page_metadata_for_project(
            input,
            &member,
            self.inventory.project(),
            max_compressed_bytes,
            max_decoded_bytes,
            |page| {
                let push = (|| {
                    self.push(PageEvent::BeginPage)?;
                    self.push(PageEvent::Title(&page.title))?;
                    self.push(PageEvent::Namespace(page.namespace))?;
                    self.push(PageEvent::PageId(page.page_id))?;
                    if let Some(target) = &page.redirect_target {
                        self.push(PageEvent::RedirectTarget(target))?;
                    }
                    self.push(PageEvent::BeginRevision)?;
                    self.push(PageEvent::RevisionId(page.revision_id))?;
                    self.push(PageEvent::Timestamp(&page.timestamp))?;
                    self.push(PageEvent::EndRevision)?;
                    self.push(PageEvent::EndPage)
                })();
                if let Err(error) = push {
                    callback_error = Some(error);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "native XML page rejected by pinned census",
                    ));
                }
                Ok(())
            },
        );
        if let Some(error) = callback_error {
            self.failed = true;
            self.ingest.abort_member();
            return Err(NativeCensusError::Census(error));
        }
        match result {
            Ok(receipt) => {
                self.end_member().map_err(NativeCensusError::Census)?;
                Ok(receipt)
            }
            Err(error) => {
                self.failed = true;
                self.ingest.abort_member();
                Err(NativeCensusError::Decode(error))
            }
        }
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
            self.observed_pages = self.observed_pages.checked_add(1).ok_or_else(|| {
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
        })?;
        self.next_member_index += 1;
        Ok(())
    }

    pub fn finish(self) -> Result<(W, usize), PinnedCensusError> {
        if self.failed {
            return Err(PinnedCensusError::InvalidState);
