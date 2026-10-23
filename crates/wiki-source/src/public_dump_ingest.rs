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
    RevisionAfterGeneration,
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
    generation_date: &'a str,
    metadata: PageMetadataCollector,
    active: bool,
    in_page: bool,
    failed: bool,
}

impl<'a> PinnedPageIngest<'a> {
    pub fn new(inventory: &'a PinnedInventory) -> Self {
        Self {
            audit: StreamShardAudit::new(inventory),
            generation_date: inventory.date(),
            metadata: PageMetadataCollector::new(),
            active: false,
            in_page: false,
            failed: false,
        }
    }

    fn project(&self) -> &str {
        self.audit.project()
    }

    fn matches_next_member(&self, member: &crate::public_dump::DumpChecksum) -> bool {
        !self.failed && !self.active && !self.in_page && self.audit.matches_next_member(member)
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
            // Revisions later than the declared source generation cannot be
            // attributed to the pinned snapshot. Timestamp syntax was checked
            // by PageMetadataCollector; fail closed before auditing/publishing.
            if &complete.timestamp[..10] > self.generation_date {
                self.failed = true;
                return Err(PageIngestError::RevisionAfterGeneration);
            }
            if let Err(error) = self.audit.push_page_id(complete.page_id) {
                self.failed = true;
                return Err(PageIngestError::Shard(error));
            }
        }
        Ok(page)
    }

    /// Re-feed a decoded page through the existing revision and shard audit.
    /// This remains provisional until the entire compressed member qualifies.
    pub fn push_complete_page(&mut self, page: &PublicDumpPage) -> Result<(), PageIngestError> {
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
        self.push(PageEvent::EndPage)?;
        Ok(())
    }

    /// A failed decoder or checksum must poison the active shard audit even
    /// if some provisional pages were observed before the failure.
    pub fn abort_member(&mut self) {
        self.failed = true;
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

/// Error from decoding and auditing one provisional bzip2 shard.
#[derive(Debug)]
pub enum PinnedDecodeError {
    UnpinnedMember,
    Ingest(PageIngestError),
    Decode(crate::public_dump_checked_read::CheckedMemberError<std::io::Error>),
}

/// Compose actual multistream bzip2 decoding, bounded XML semantics and
/// pinned shard ordering. A checksum/EOF/semantic failure poisons the audit.
///
/// This is not independently authenticated publication completeness or a
/// renderer. Do not activate output until every member and the independent
/// census, source identity and required offline media have qualified.
pub fn ingest_pinned_bzip2_member<R: std::io::Read>(
    ingest: &mut PinnedPageIngest<'_>,
    member: &crate::public_dump::DumpChecksum,
    input: R,
    max_compressed_bytes: u64,
    max_decoded_bytes: u64,
) -> Result<(u64, u64), PinnedDecodeError> {
    // A caller-supplied descriptor with a correct filename but forged SHA-256
    // must not override the inventory's authenticated digest.
    if !ingest.matches_next_member(member) {
        ingest.abort_member();
        return Err(PinnedDecodeError::UnpinnedMember);
    }
    ingest
        .begin_member(&member.relative_path)
        .map_err(PinnedDecodeError::Ingest)?;
    let mut callback_error = None;
    let project = ingest.project().to_owned();
    let decoded = crate::public_dump_bzip2::decode_bzip2_xml_page_metadata_for_project(
        input,
        member,
        &project,
        max_compressed_bytes,
        max_decoded_bytes,
        |page| {
            if let Err(error) = ingest.push_complete_page(&page) {
                callback_error = Some(error);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "pinned shard rejected decoded page",
                ));
            }
            Ok(())
        },
    );
    if let Some(error) = callback_error {
        ingest.abort_member();
        return Err(PinnedDecodeError::Ingest(error));
    }
    match decoded {
        Ok(receipt) => {
            ingest.end_member().map_err(PinnedDecodeError::Ingest)?;
            Ok(receipt)
        }
        Err(error) => {
            ingest.abort_member();
            Err(PinnedDecodeError::Decode(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};

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
        ingest.push(PageEvent::Timestamp("2026-10-01T12:00:00Z"))?;
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
    fn refuses_revision_after_declared_snapshot_date_permanently() {
        let pinned = inventory();
        let mut ingest = PinnedPageIngest::new(&pinned);
        ingest
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        ingest.push(PageEvent::BeginPage).unwrap();
        ingest.push(PageEvent::Title("Later revision")).unwrap();
        ingest.push(PageEvent::Namespace(0)).unwrap();
        ingest.push(PageEvent::PageId(1)).unwrap();
        ingest.push(PageEvent::BeginRevision).unwrap();
        ingest.push(PageEvent::RevisionId(42)).unwrap();
        ingest
            .push(PageEvent::Timestamp("2026-10-02T00:00:00Z"))
            .unwrap();
        ingest.push(PageEvent::EndRevision).unwrap();
        assert_eq!(
            ingest.push(PageEvent::EndPage),
            Err(PageIngestError::RevisionAfterGeneration)
        );
        assert_eq!(ingest.end_member(), Err(PageIngestError::InvalidState));
        assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));
    }

    #[test]
    fn native_bzip2_metadata_feeds_pinned_shard_audit() {
        let xml = "<mediawiki><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>22</id><timestamp>2026-10-01T12:00:00Z</timestamp><contributor><id>999</id></contributor><text>test</text></revision></page></mediawiki>";
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        let checksum = format!("{:x}", Sha256::digest(&compressed));
        let sums = format!("{checksum}  enwiki-2026-10-01-p1p100.xml.bz2\n");
        let sums_digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let inventory =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &sums_digest).unwrap();
        let mut ingest = PinnedPageIngest::new(&inventory);
        let (decoded, count) = ingest_pinned_bzip2_member(
            &mut ingest,
            &inventory.members()[0],
            Cursor::new(&compressed),
            4096,
            4096,
        )
        .unwrap();
        assert_eq!(decoded, xml.len() as u64);
        assert_eq!(count, 1);
        assert_eq!(ingest.finish(), Ok(1));
    }

    #[test]
    fn pinned_native_ingest_rejects_forged_siteinfo_database_identity() {
        for (database, accepted) in [("frwiki", false), ("enwiki", true)] {
            let xml = format!(
                "<mediawiki><siteinfo><dbname>{database}</dbname></siteinfo><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>22</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>"
            );
            let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
            encoder.write_all(xml.as_bytes()).unwrap();
            let compressed = encoder.finish().unwrap();
            let sums = format!(
                "{:x}  enwiki-2026-10-01-p1p100.xml.bz2\n",
                Sha256::digest(&compressed)
            );
            let pinned = parse_pinned_inventory(
                sums.as_bytes(),
                "enwiki",
                "2026-10-01",
                &format!("{:x}", Sha256::digest(sums.as_bytes())),
            )
            .unwrap();
            let mut ingest = PinnedPageIngest::new(&pinned);
            let result = ingest_pinned_bzip2_member(
                &mut ingest,
                &pinned.members()[0],
                Cursor::new(&compressed),
                4096,
                4096,
            );
            if accepted {
                assert_eq!(result.unwrap().1, 1);
                assert_eq!(ingest.finish(), Ok(1));
            } else {
                assert!(matches!(result, Err(PinnedDecodeError::Decode(_))));
                assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));
            }
        }
    }

    #[test]
    fn digest_mismatch_poisoning_prevents_false_shard_completion() {
        let xml = "<mediawiki><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>22</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>";
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        let sums = format!("{}  enwiki-2026-10-01-p1p100.xml.bz2\n", "a".repeat(64));
        let sums_digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let inventory =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &sums_digest).unwrap();
        let mut ingest = PinnedPageIngest::new(&inventory);
        let result = ingest_pinned_bzip2_member(
            &mut ingest,
            &inventory.members()[0],
            Cursor::new(&compressed),
            4096,
            4096,
        );
        assert!(matches!(
            result,
            Err(PinnedDecodeError::Decode(
                crate::public_dump_checked_read::CheckedMemberError::DigestMismatch
            ))
        ));
        assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));
    }

    #[test]
    fn rejects_caller_forged_checksum_before_decoding_or_publishing() {
        let pinned = inventory();
        let expected = &pinned.members()[0];
        let mut forged = expected.clone();
        forged.sha256 = "f".repeat(64);
        assert_ne!(forged.sha256, expected.sha256);
        let mut ingest = PinnedPageIngest::new(&pinned);
        assert!(matches!(
            ingest_pinned_bzip2_member(
                &mut ingest,
                &forged,
                Cursor::new(b"not even bzip2"),
                4096,
                4096,
            ),
            Err(PinnedDecodeError::UnpinnedMember)
        ));
        assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));

        let mut wrong_path = expected.clone();
        wrong_path.relative_path = pinned.members()[1].relative_path.clone();
        let mut ingest = PinnedPageIngest::new(&pinned);
        assert!(matches!(
            ingest_pinned_bzip2_member(&mut ingest, &wrong_path, Cursor::new(b""), 4096, 4096,),
            Err(PinnedDecodeError::UnpinnedMember)
        ));
        assert_eq!(ingest.finish(), Err(PageIngestError::InvalidState));
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
