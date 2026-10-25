//! Fail-closed bridge between validated pinned dump events and census export.
//!
//! A caller must still authenticate and verify compressed source members, decode
//! well-formed XML, and stage the output privately before atomic publication.
//! This bridge cannot supply an *independent* expected page census.

use std::io::{Read, Write};

use sha2::{Digest, Sha256};

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
    ExpectedCensusChanged,
    MemberCensusMismatch {
        member: String,
        expected_count: u64,
        observed_count: u64,
    },
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
    namespace: i32,
    member_hash: Sha256,
    member_count: u64,
    last_member_receipt: Option<(String, u64)>,
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
            namespace,
            member_hash: Sha256::new(),
            member_count: 0,
            last_member_receipt: None,
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
        })?;
        self.member_hash = Sha256::new();
        self.member_count = 0;
        self.last_member_receipt = None;
        Ok(())
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
            if page.namespace == self.namespace {
                // Hash exactly the decimal/LF bytes emitted for this namespace.
                // All other namespaces still participate in shard ordering.
                self.member_hash.update(page.page_id.to_string().as_bytes());
                self.member_hash.update(b"\n");
                self.member_count = self.member_count.checked_add(1).ok_or_else(|| {
                    self.failed = true;
                    PinnedCensusError::CountMismatch
                })?;
            }
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
        self.last_member_receipt = Some((
            format!("{:x}", self.member_hash.clone().finalize()),
            self.member_count,
        ));
        Ok(())
    }

    /// The digest/count of the last completed member's selected-namespace
    /// census. Never exposes a partially decoded or failed member.
    pub fn last_member_census_receipt(&self) -> Option<(&str, u64)> {
        if self.failed {
            return None;
        }
        self.last_member_receipt
            .as_ref()
            .map(|(digest, count)| (digest.as_str(), *count))
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

/// Stage a complete observed namespace census from real, inventory-pinned
/// bzip2/XML members, without exposing the manual semantic-event feed seam.
///
/// `open_member` supplies a readable stream for each inventory path in order.
/// Every stream is decoded against the inventory's own digest and bounded by
/// both byte budgets. A failed open, decode, semantic audit, or final flush
/// removes the private staging file and cannot publish a partial census.
///
/// This is still an *observed* census, not independently authenticated expected
/// coverage or a qualified source snapshot. The caller must authenticate the
/// inventory and independently audit source completeness before activation.
pub fn stage_pinned_native_bzip2_census<R, F>(
    root: &std::path::Path,
    filename: &str,
    inventory: &PinnedInventory,
    namespace: i32,
    max_compressed_bytes: u64,
    max_decoded_bytes: u64,
    mut open_member: F,
) -> Result<(std::path::PathBuf, usize), CensusStageError>
where
    R: Read,
    F: FnMut(&str) -> std::io::Result<R>,
{
    stage_pinned_observed_census(root, filename, inventory, namespace, |pipeline| {
        for member in inventory.members() {
            let input = open_member(&member.relative_path).map_err(PinnedCensusError::Source)?;
            pipeline
                .decode_native_bzip2_member(input, max_compressed_bytes, max_decoded_bytes)
                .map_err(|error| match error {
                    NativeCensusError::Census(error) => error,
                    NativeCensusError::Decode(error) => PinnedCensusError::NativeDecode(error),
                })?;
        }
        Ok(())
    })
}

/// Publish an observed census from validated semantic events using a private,
/// no-overwrite staging file. The callback must feed *all* pinned members and
/// must only emit events from independently checksum-verified, well-formed XML.
///
/// This is NOT a source authenticator or an independent completeness audit.
/// The staging directory must be private to the caller: path validation cannot
/// defend against an adversary concurrently replacing directory components.
pub fn stage_pinned_observed_census<F>(
    root: &std::path::Path,
    filename: &str,
    inventory: &PinnedInventory,
    namespace: i32,
    feed: F,
) -> Result<(std::path::PathBuf, usize), CensusStageError>
where
    F: FnOnce(&mut PinnedObservedCensus<'_, std::fs::File>) -> Result<(), PinnedCensusError>,
{
    use std::fs::{self, File, OpenOptions};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    if filename.is_empty()
        || filename.len() > 255
        || filename.starts_with('.')
        || filename.ends_with('.')
        || crate::windows_device_name(filename)
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(CensusStageError::UnsafePath);
    }
    for component in root.ancestors() {
        let metadata = fs::symlink_metadata(component).map_err(CensusStageError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(CensusStageError::UnsafePath);
        }
    }
    let metadata = fs::symlink_metadata(root).map_err(CensusStageError::Io)?;
    if !metadata.file_type().is_dir() {
        return Err(CensusStageError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // The caller-controlled staging root must not expose partial census
        // bytes to other users or permit directory-entry substitution.
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CensusStageError::UnsafePath);
        }
    }
    let target = root.join(filename);
    let (temporary, output) = loop {
        let candidate = root.join(format!(
            ".wiki-census-{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(output) => break (candidate, output),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CensusStageError::Io(error)),
        }
    };
    let result = (|| -> Result<usize, CensusStageError> {
        let mut pipeline = PinnedObservedCensus::new(inventory, output, namespace)
            .map_err(CensusStageError::Census)?;
        feed(&mut pipeline).map_err(CensusStageError::Census)?;
        let (output, count) = pipeline.finish().map_err(CensusStageError::Census)?;
        output.sync_all().map_err(CensusStageError::Io)?;
        // hard_link is atomic and refuses to overwrite an existing census.
        fs::hard_link(&temporary, &target).map_err(CensusStageError::Io)?;
        if let Err(error) = fs::remove_file(&temporary) {
            let _ = fs::remove_file(&target);
            return Err(CensusStageError::Io(error));
        }
        #[cfg(unix)]
        if let Err(error) = File::open(root).and_then(|directory| directory.sync_all()) {
            let _ = fs::remove_file(&target);
            return Err(CensusStageError::Io(error));
        }
        Ok(count)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|count| (target, count))
}

#[derive(Debug)]
pub enum CensusStageError {
    UnsafePath,
    Census(PinnedCensusError),
    Io(std::io::Error),
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

    #[test]
    fn member_census_receipts_track_only_selected_namespace_and_complete_members() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        pipeline
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        assert_eq!(pipeline.last_member_census_receipt(), None);
        push_page(&mut pipeline, 1, 1).unwrap();
        push_page(&mut pipeline, 2, 0).unwrap();
        pipeline.end_member().unwrap();
        assert_eq!(
            pipeline.last_member_census_receipt(),
            Some((format!("{:x}", Sha256::digest(b"2\n")).as_str(), 1))
        );
        pipeline
            .begin_member(&pinned.members()[1].relative_path)
            .unwrap();
        assert_eq!(pipeline.last_member_census_receipt(), None);
        push_page(&mut pipeline, 201, 1).unwrap();
        pipeline.end_member().unwrap();
        assert_eq!(
            pipeline.last_member_census_receipt(),
            Some((format!("{:x}", Sha256::digest(b"")).as_str(), 0))
        );
        let (bytes, count) = pipeline.finish().unwrap();
        assert_eq!(count, 1);
        assert_eq!(bytes, b"2\n");
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
        pipeline.push(PageEvent::Timestamp("2026-10-01T12:00:00Z"))?;
        pipeline.push(PageEvent::EndRevision)?;
        pipeline.push(PageEvent::EndPage)
    }

    #[test]
    fn native_staging_rejects_wrong_siteinfo_project_without_publication() {
        use std::fs;
        use std::io::Cursor;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "wiki-native-project-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let xml = "<mediawiki><siteinfo><dbname>frwiki</dbname></siteinfo><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>2</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>";
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let bytes = encoder.finish().unwrap();
        let sums = format!(
            "{:x}  enwiki-2026-10-01-p1p100.xml.bz2\n",
            Sha256::digest(&bytes)
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let pinned =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let result =
            stage_pinned_native_bzip2_census(&root, "observed.ids", &pinned, 0, 4096, 4096, |_| {
                Ok(Cursor::new(bytes.as_slice()))
            });
        assert!(matches!(
            result,
            Err(CensusStageError::Census(PinnedCensusError::NativeDecode(_)))
        ));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_staging_decodes_all_pinned_members_and_never_publishes_failed_shards() {
        use std::fs;
        use std::io::Cursor;
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "wiki-native-census-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }

        let xml = [
            "<mediawiki><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>1001</id><timestamp>2026-10-01T12:00:00Z</timestamp><contributor><id>9001</id></contributor></revision></page></mediawiki>",
            "<mediawiki><page><title>Biology</title><ns>0</ns><id>201</id><revision><id>1201</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
        ];
        let compressed: Vec<Vec<u8>> = xml
            .iter()
            .map(|text| {
                let mut encoder =
                    bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
                encoder.write_all(text.as_bytes()).unwrap();
                encoder.finish().unwrap()
            })
            .collect();
        let sums = format!(
            "{:x}  enwiki-2026-10-01-p1p100.xml.bz2\n{:x}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            Sha256::digest(&compressed[0]),
            Sha256::digest(&compressed[1])
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let pinned =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let open = |path: &str| {
            let index = pinned
                .members()
                .iter()
                .position(|member| member.relative_path == path)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "unknown member"))?;
            Ok(Cursor::new(compressed[index].as_slice()))
        };
        let (published, count) =
            stage_pinned_native_bzip2_census(&root, "native.ids", &pinned, 0, 4096, 4096, open)
                .unwrap();
        assert_eq!(count, 2);
        assert_eq!(fs::read(&published).unwrap(), b"1\n201\n");

        // The first shard is valid, but the second has a forged pinned digest.
        // Its failure must leave no second published file or temporary bytes.
        let bad_sums = format!(
            "{:x}  enwiki-2026-10-01-p1p100.xml.bz2\n{}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            Sha256::digest(&compressed[0]),
            "0".repeat(64)
        );
        let bad_digest = format!("{:x}", Sha256::digest(bad_sums.as_bytes()));
        let bad_pinned =
            parse_pinned_inventory(bad_sums.as_bytes(), "enwiki", "2026-10-01", &bad_digest)
                .unwrap();
        assert!(matches!(
            stage_pinned_native_bzip2_census(
                &root,
                "bad.ids",
                &bad_pinned,
                0,
                4096,
                4096,
                |path| {
                    let index = bad_pinned
                        .members()
                        .iter()
                        .position(|member| member.relative_path == path)
                        .unwrap();
                    Ok(Cursor::new(compressed[index].as_slice()))
                }
            ),
            Err(CensusStageError::Census(PinnedCensusError::NativeDecode(
                CheckedMemberError::DigestMismatch
            )))
        ));
        assert!(matches!(
            stage_pinned_native_bzip2_census(
                &root,
                "unavailable.ids",
                &pinned,
                0,
                4096,
                4096,
                |_path| Err::<Cursor<&[u8]>, _>(io::Error::new(
                    io::ErrorKind::NotFound,
                    "source member unavailable"
                ))
            ),
            Err(CensusStageError::Census(PinnedCensusError::Source(error)))
                if error.kind() == io::ErrorKind::NotFound
        ));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(fs::read(&published).unwrap(), b"1\n201\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_multistream_bzip2_produces_staged_two_shard_census() {
        use std::io::Cursor;

        let xml = [
            "<mediawiki><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>1001</id><timestamp>2026-10-01T12:00:00Z</timestamp><contributor><id>9001</id></contributor><text><![CDATA[{{x}}]]></text></revision></page></mediawiki>",
            "<mediawiki><page><title>Biology</title><ns>0</ns><id>201</id><revision><id>1201</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
        ];
        let compressed: Vec<Vec<u8>> = xml
            .iter()
            .map(|text| {
                let mut encoder =
                    bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
                encoder.write_all(text.as_bytes()).unwrap();
                encoder.finish().unwrap()
            })
            .collect();
        let sums = format!(
            "{:x}  enwiki-2026-10-01-p1p100.xml.bz2\n{:x}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            Sha256::digest(&compressed[0]),
            Sha256::digest(&compressed[1])
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let pinned =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        for (index, bytes) in compressed.iter().enumerate() {
            let (decoded, count) = pipeline
                .decode_native_bzip2_member(Cursor::new(bytes), 4096, 4096)
                .unwrap();
            assert_eq!(decoded, xml[index].len() as u64);
            assert_eq!(count, 1);
        }
        let (bytes, count) = pipeline.finish().unwrap();
        assert_eq!(bytes, b"1\n201\n");
        assert_eq!(count, 2);
    }

    #[test]
    fn native_census_digest_failure_cannot_return_partial_census() {
        use std::io::Cursor;

        let xml = "<mediawiki><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>1001</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>";
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
        encoder.write_all(xml.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        let sums = format!("{}  enwiki-2026-10-01-p1p100.xml.bz2\n", "a".repeat(64));
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let pinned =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        assert!(matches!(
            pipeline.decode_native_bzip2_member(Cursor::new(&compressed), 4096, 4096),
            Err(NativeCensusError::Decode(
                CheckedMemberError::DigestMismatch
            ))
        ));
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::InvalidState)
        ));
    }

    #[test]
    fn checked_compressed_members_can_complete_the_pinned_census() {
        use std::io::Cursor;

        let payloads: [&[u8]; 2] = [b"compressed first fixture", b"compressed second fixture"];
        let sums = format!(
            "{:x}  enwiki-2026-10-01-p1p100.xml.bz2\n{:x}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            Sha256::digest(payloads[0]),
            Sha256::digest(payloads[1])
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let pinned =
            parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        for (index, payload) in payloads.into_iter().enumerate() {
            pipeline
                .decode_checked_member(Cursor::new(payload), 1024, |reader, census| {
                    let mut consumed = Vec::new();
                    reader
                        .read_to_end(&mut consumed)
                        .map_err(|_| PinnedCensusError::InvalidState)?;
                    assert_eq!(consumed, payload);
                    push_page(census, if index == 0 { 1 } else { 201 }, 0)
                })
                .unwrap();
        }
        let (bytes, count) = pipeline.finish().unwrap();
        assert_eq!(bytes, b"1\n201\n");
        assert_eq!(count, 2);
    }

    #[test]
    fn failed_compressed_receipt_poisoned_even_after_valid_page_events() {
        use std::io::Cursor;

        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        let bad = pipeline.decode_checked_member(
            Cursor::new(b"not a valid pinned member"),
            1024,
            |reader, census| {
                let mut bytes = Vec::new();
                reader
                    .read_to_end(&mut bytes)
                    .map_err(|_| PinnedCensusError::InvalidState)?;
                push_page(census, 1, 0)
            },
        );
        assert!(matches!(
            bad,
            Err(CheckedCensusError::Member(
                CheckedMemberError::DigestMismatch
            ))
        ));
        assert!(matches!(
            pipeline.begin_member(&pinned.members()[1].relative_path),
            Err(PinnedCensusError::InvalidState)
        ));
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::InvalidState)
        ));

        let mut early = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        assert!(matches!(
            early.decode_checked_member(
                Cursor::new(b"unexpected trailing data"),
                1024,
                |_reader, census| push_page(census, 1, 0),
            ),
            Err(CheckedCensusError::Member(
                CheckedMemberError::IncompleteConsumption
            ))
        ));
        assert!(matches!(
            early.finish(),
            Err(PinnedCensusError::InvalidState)
        ));
    }

    #[test]
    fn validated_multi_shard_pages_produce_only_selected_namespace_ids() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        pipeline
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        push_page(&mut pipeline, 1, 0).unwrap();
        push_page(&mut pipeline, 2, 1).unwrap();
        pipeline.end_member().unwrap();
        pipeline
            .begin_member(&pinned.members()[1].relative_path)
            .unwrap();
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
        pipeline
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        push_page(&mut pipeline, 1, 0).unwrap();
        pipeline.end_member().unwrap();
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::Ingest(_))
        ));

        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        pipeline
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        push_page(&mut pipeline, 1, 0).unwrap();
        assert!(matches!(
            push_page(&mut pipeline, 1, 0),
            Err(PinnedCensusError::Ingest(_))
        ));
        assert!(matches!(
            pipeline.end_member(),
            Err(PinnedCensusError::InvalidState)
        ));
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::InvalidState)
        ));
    }

    #[test]
    fn empty_selected_namespace_is_not_published_even_after_full_shard_audit() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, Vec::new(), 0).unwrap();
        for member in pinned.members() {
            pipeline.begin_member(&member.relative_path).unwrap();
            let id = if member.relative_path.contains("p201p300") {
                201
            } else {
                1
            };
            push_page(&mut pipeline, id, 1).unwrap();
            pipeline.end_member().unwrap();
        }
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::Export(CensusExportError::EmptyCensus))
        ));
    }

    struct RejectingFlush {
        bytes: Vec<u8>,
    }

    impl Write for RejectingFlush {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "staging flush failed",
            ))
        }
    }

    #[test]
    fn flush_error_after_valid_shards_never_returns_export_bytes() {
        let pinned = inventory();
        let mut pipeline =
            PinnedObservedCensus::new(&pinned, RejectingFlush { bytes: Vec::new() }, 0).unwrap();
        for member in pinned.members() {
            pipeline.begin_member(&member.relative_path).unwrap();
            let id = if member.relative_path.contains("p201p300") {
                201
            } else {
                1
            };
            push_page(&mut pipeline, id, 0).unwrap();
            pipeline.end_member().unwrap();
        }
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::Export(CensusExportError::Write(_)))
        ));
    }

    struct RejectingWriter;

    impl Write for RejectingWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "closed staging sink",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn staging_write_failure_poisons_entire_pinned_ingestion() {
        let pinned = inventory();
        let mut pipeline = PinnedObservedCensus::new(&pinned, RejectingWriter, 0).unwrap();
        pipeline
            .begin_member(&pinned.members()[0].relative_path)
            .unwrap();
        assert!(matches!(
            push_page(&mut pipeline, 1, 0),
            Err(PinnedCensusError::Export(CensusExportError::Write(_)))
        ));
        assert!(matches!(
            pipeline.end_member(),
            Err(PinnedCensusError::InvalidState)
        ));
        assert!(matches!(
            pipeline.finish(),
            Err(PinnedCensusError::InvalidState)
        ));
    }

    #[test]
    fn private_staging_publishes_only_complete_census_and_refuses_overwrite() {
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "wiki-census-stage-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let pinned = inventory();
        let complete = |pipeline: &mut PinnedObservedCensus<'_, fs::File>| {
            for (index, member) in pinned.members().iter().enumerate() {
                pipeline.begin_member(&member.relative_path)?;
                push_page(pipeline, if index == 0 { 1 } else { 201 }, 0)?;
                pipeline.end_member()?;
            }
            Ok(())
        };
        let (published, count) =
            stage_pinned_observed_census(&root, "observed.ids", &pinned, 0, complete).unwrap();
        assert_eq!(count, 2);
        assert_eq!(fs::read(&published).unwrap(), b"1\n201\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&published).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(matches!(
            stage_pinned_observed_census(&root, "observed.ids", &pinned, 0, complete),
            Err(CensusStageError::Io(error))
                if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(fs::read(&published).unwrap(), b"1\n201\n");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_or_incomplete_ingest_never_publishes_a_census() {
        use std::fs;
        let root = std::env::temp_dir().join(format!(
            "wiki-census-fail-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let pinned = inventory();
        assert!(matches!(
            stage_pinned_observed_census(&root, "incomplete.ids", &pinned, 0, |pipeline| {
                pipeline.begin_member(&pinned.members()[0].relative_path)?;
                push_page(pipeline, 1, 0)?;
                pipeline.end_member()
            }),
            Err(CensusStageError::Census(PinnedCensusError::Ingest(_)))
        ));
        assert!(matches!(
            stage_pinned_observed_census(&root, "rejected.ids", &pinned, 0, |pipeline| {
                pipeline.begin_member(&pinned.members()[0].relative_path)?;
                push_page(pipeline, 1, 0)?;
                push_page(pipeline, 1, 0)
            }),
            Err(CensusStageError::Census(PinnedCensusError::Ingest(_)))
        ));
        for bad in ["../escape", "CON", ".hidden", "file/name", "a\\b"] {
            assert!(matches!(
                stage_pinned_observed_census(&root, bad, &pinned, 0, |_| Ok(())),
                Err(CensusStageError::UnsafePath)
            ));
        }
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_staging_root_is_rejected_before_file_creation() {
        use std::fs;
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("wiki-census-link-{}", std::process::id()));
        let linked = root.with_extension("link");
        fs::create_dir(&root).unwrap();
        symlink(&root, &linked).unwrap();
        assert!(matches!(
            stage_pinned_observed_census(&linked, "observed.ids", &inventory(), 0, |_| Ok(())),
            Err(CensusStageError::UnsafePath)
        ));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_file(linked).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn shared_staging_directory_is_rejected_before_writing() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("wiki-census-public-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            stage_pinned_observed_census(&root, "observed.ids", &inventory(), 0, |_| Ok(())),
            Err(CensusStageError::UnsafePath)
        ));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}
