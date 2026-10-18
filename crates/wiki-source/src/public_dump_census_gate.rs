//! Digest-pinned, bounded-memory comparison of an independently supplied
//! expected page census with the observed census from verified dump ingestion.
//!
//! A caller MUST separately authenticate the expected census publisher, its
//! snapshot cutoff and namespace semantics. A checksum supplied by that same
//! caller is an integrity pin, not proof of Wikimedia origin or independence.
//! This gate does not decode XML, render wikitext, or certify offline packs.

use std::fs::{self, File, Metadata};
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::public_dump_census_stream::{CensusScope, StreamCoverageError};
use crate::public_dump_census_text::audit_page_coverage_text;
use crate::public_dump_coverage::PageCoverageDelta;
use crate::public_dump_pinned::PinnedInventory;

const MAX_CENSUS_BYTES: u64 = 2_000_000_000;

#[derive(Debug)]
pub enum CensusGateError {
    InvalidDigest,
    UnsafeInput,
    SameSource,
    Io(io::Error),
    ExpectedDigestMismatch,
    Coverage(StreamCoverageError),
    ChangedDuringAudit,
}

fn open_regular(path: &Path) -> Result<(File, Metadata), CensusGateError> {
    let link = fs::symlink_metadata(path).map_err(CensusGateError::Io)?;
    if !link.file_type().is_file() || link.len() == 0 || link.len() > MAX_CENSUS_BYTES {
        return Err(CensusGateError::UnsafeInput);
    }
    let file = File::open(path).map_err(CensusGateError::Io)?;
    let opened = file.metadata().map_err(CensusGateError::Io)?;
    if !opened.is_file() || !same_identity(&link, &opened) {
        return Err(CensusGateError::UnsafeInput);
    }
    Ok((file, opened))
}

fn same_identity(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        a.len() == b.len() && a.modified().ok() == b.modified().ok()
    }
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    if !same_identity(before, after)
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn sha256_file(file: &mut File) -> Result<String, CensusGateError> {
    file.seek(SeekFrom::Start(0)).map_err(CensusGateError::Io)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let size = file.read(&mut buffer).map_err(CensusGateError::Io)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    file.seek(SeekFrom::Start(0)).map_err(CensusGateError::Io)?;
    Ok(format!("{:x}", hash.finalize()))
}

/// Audit observed IDs against a separately authenticated, SHA-256-pinned
/// expected census for the same pinned project/generation and namespace.
///
/// Both files must be regular, non-symlink files and may not refer to the same
/// inode. A successful comparison is not proof that the expected census is
/// independent or has a compatible snapshot cutoff: callers must establish
/// those facts out of band. Both readers use bounded memory and strict LF
/// decimal parsing. Expected bytes are hashed before and after comparison.
pub fn audit_pinned_census_files(
    inventory: &PinnedInventory,
    namespace: i32,
    expected_path: &Path,
    expected_sha256: &str,
    observed_path: &Path,
) -> Result<PageCoverageDelta, CensusGateError> {
    if namespace < 0 {
        return Err(CensusGateError::UnsafeInput);
    }
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(CensusGateError::InvalidDigest);
    }
    let (mut expected, expected_before) = open_regular(expected_path)?;
    let (mut observed, observed_before) = open_regular(observed_path)?;
    if same_identity(&expected_before, &observed_before)
        || fs::canonicalize(expected_path).map_err(CensusGateError::Io)?
            == fs::canonicalize(observed_path).map_err(CensusGateError::Io)?
    {
        return Err(CensusGateError::SameSource);
    }
    if !sha256_file(&mut expected)?.eq_ignore_ascii_case(expected_sha256) {
        return Err(CensusGateError::ExpectedDigestMismatch);
    }
    let scope = CensusScope {
        project: inventory.project(),
        generation: inventory.date(),
        namespace,
    };
    let delta = audit_page_coverage_text(
        scope,
        BufReader::new(&mut expected),
        scope,
        BufReader::new(&mut observed),
    )
    .map_err(CensusGateError::Coverage)?;
    if !sha256_file(&mut expected)?.eq_ignore_ascii_case(expected_sha256) {
        return Err(CensusGateError::ChangedDuringAudit);
    }
    let expected_after = expected.metadata().map_err(CensusGateError::Io)?;
    let observed_after = observed.metadata().map_err(CensusGateError::Io)?;
    if !unchanged(&expected_before, &expected_after)
        || !unchanged(&observed_before, &observed_after)
    {
        return Err(CensusGateError::ChangedDuringAudit);
    }
    Ok(delta)
}

#[derive(Debug)]
pub enum VerifiedCensusStageError {
    UnsafePath,
    Stage(crate::public_dump_census_ingest::CensusStageError),
    Audit(CensusGateError),
    Io(io::Error),
    CountMismatch,
}

/// Stage all pinned semantic events in a private directory and expose the
/// requested final filename ONLY after the digest-pinned census audit passes.
/// The caller must separately authenticate and establish independence/cutoff
/// semantics for the expected census. The callback must feed verified XML.
pub fn stage_and_audit_pinned_census<F>(
    root: &Path,
    filename: &str,
    inventory: &PinnedInventory,
    namespace: i32,
    expected_path: &Path,
    expected_sha256: &str,
    feed: F,
) -> Result<(std::path::PathBuf, PageCoverageDelta), VerifiedCensusStageError>
where
    F: FnOnce(
        &mut crate::public_dump_census_ingest::PinnedObservedCensus<'_, File>,
    ) -> Result<(), crate::public_dump_census_ingest::PinnedCensusError>,
{
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_PENDING: AtomicU64 = AtomicU64::new(0);

    if filename.is_empty()
        || filename.len() > 255
        || filename.starts_with('.')
        || filename.ends_with('.')
        || crate::windows_device_name(filename)
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(VerifiedCensusStageError::UnsafePath);
    }
    let pending_name = format!(
        "pending-census-{}-{}.ids",
        std::process::id(),
        NEXT_PENDING.fetch_add(1, Ordering::Relaxed)
    );
    let (pending, count) = crate::public_dump_census_ingest::stage_pinned_observed_census(
        root,
        &pending_name,
        inventory,
        namespace,
        feed,
    )
    .map_err(VerifiedCensusStageError::Stage)?;
    let destination = root.join(filename);
    let mut published = false;
    let result = (|| {
        let delta = audit_pinned_census_files(
            inventory,
            namespace,
            expected_path,
            expected_sha256,
            &pending,
        )
        .map_err(VerifiedCensusStageError::Audit)?;
        if delta.observed_count != count {
            return Err(VerifiedCensusStageError::CountMismatch);
        }
        // Same-filesystem hard link atomically refuses an existing destination.
        fs::hard_link(&pending, &destination).map_err(VerifiedCensusStageError::Io)?;
        published = true;
        fs::remove_file(&pending).map_err(VerifiedCensusStageError::Io)?;
        #[cfg(unix)]
        File::open(root)
            .and_then(|dir| dir.sync_all())
            .map_err(VerifiedCensusStageError::Io)?;
        Ok((destination.clone(), delta))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
        if published {
            let _ = fs::remove_file(&destination);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn inventory() -> PinnedInventory {
        let sums = format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n{}  enwiki-2026-10-01-p201p300.xml.bz2\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(sums.as_bytes()));
        parse_pinned_inventory(sums.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap()
    }

    fn fixture() -> (
        std::path::PathBuf,
        std::path::PathBuf,
        std::path::PathBuf,
        String,
    ) {
        let root = std::env::temp_dir().join(format!(
            "wiki-census-gate-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let expected = root.join("expected.ids");
        let observed = root.join("observed.ids");
        fs::write(&expected, b"1\n201\n").unwrap();
        fs::write(&observed, b"1\n201\n").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"1\n201\n"));
        (root, expected, observed, digest)
    }

    #[test]
    fn pinned_independent_census_matches_without_materializing_ids() {
        let (root, expected, observed, digest) = fixture();
        let delta =
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed).unwrap();
        assert_eq!(delta.expected_count, 2);
        assert_eq!(delta.observed_count, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_unpinned_expected_bytes_and_mismatched_page_sets() {
        let (root, expected, observed, digest) = fixture();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &"0".repeat(64), &observed),
            Err(CensusGateError::ExpectedDigestMismatch)
        ));
        fs::write(&observed, b"1\n202\n").unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed),
            Err(CensusGateError::Coverage(
                StreamCoverageError::MissingOrUnexpected(_)
            ))
        ));
        fs::write(&expected, b"1\n201\nextra\n").unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed),
            Err(CensusGateError::ExpectedDigestMismatch)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_aliases_malformed_inputs_and_invalid_scopes() {
        let (root, expected, observed, digest) = fixture();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &expected),
            Err(CensusGateError::SameSource)
        ));
        assert!(matches!(
            audit_pinned_census_files(&inventory(), -1, &expected, &digest, &observed),
            Err(CensusGateError::UnsafeInput)
        ));
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, "bad", &observed),
            Err(CensusGateError::InvalidDigest)
        ));
        fs::write(&observed, b"1\n201").unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed),
            Err(CensusGateError::Coverage(StreamCoverageError::Read { .. }))
        ));
        fs::write(&observed, b"").unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed),
            Err(CensusGateError::UnsafeInput)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_and_hardlink_aliases() {
        use std::os::unix::fs::symlink;
        let (root, expected, observed, digest) = fixture();
        let linked = root.join("linked.ids");
        symlink(&expected, &linked).unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &linked, &digest, &observed),
            Err(CensusGateError::UnsafeInput)
        ));
        fs::remove_file(&linked).unwrap();
        fs::hard_link(&expected, &linked).unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &linked),
            Err(CensusGateError::SameSource)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_pinned_events_roundtrip_through_digest_pinned_coverage_gate() {
        use crate::public_dump_census_ingest::stage_pinned_observed_census;
        use crate::public_dump_page_meta::PageEvent;
        let root = std::env::temp_dir().join(format!(
            "wiki-census-integration-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let expected = root.join("expected.ids");
        fs::write(&expected, b"1\n201\n").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"1\n201\n"));
        let pinned = inventory();
        let (observed, count) =
            stage_pinned_observed_census(&root, "observed.ids", &pinned, 0, |pipeline| {
                for (index, member) in pinned.members().iter().enumerate() {
                    pipeline.begin_member(&member.relative_path)?;
                    pipeline.push(PageEvent::BeginPage)?;
                    pipeline.push(PageEvent::Title("Example"))?;
                    pipeline.push(PageEvent::Namespace(0))?;
                    pipeline.push(PageEvent::PageId(if index == 0 { 1 } else { 201 }))?;
                    pipeline.push(PageEvent::BeginRevision)?;
                    pipeline.push(PageEvent::RevisionId(1001 + index as u64))?;
                    pipeline.push(PageEvent::Timestamp("2026-10-01T12:00:00Z"))?;
                    pipeline.push(PageEvent::EndRevision)?;
                    pipeline.push(PageEvent::EndPage)?;
                    pipeline.end_member()?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(count, 2);
        let result = audit_pinned_census_files(&pinned, 0, &expected, &digest, &observed).unwrap();
        assert_eq!(result.expected_count, count);
        assert_eq!(result.observed_count, count);
        fs::remove_dir_all(root).unwrap();
    }

    fn feed_two_pinned_pages(
        pinned: &PinnedInventory,
        pipeline: &mut crate::public_dump_census_ingest::PinnedObservedCensus<'_, File>,
    ) -> Result<(), crate::public_dump_census_ingest::PinnedCensusError> {
        use crate::public_dump_page_meta::PageEvent;
        for (index, member) in pinned.members().iter().enumerate() {
            pipeline.begin_member(&member.relative_path)?;
            pipeline.push(PageEvent::BeginPage)?;
            pipeline.push(PageEvent::Title("Example"))?;
            pipeline.push(PageEvent::Namespace(0))?;
            pipeline.push(PageEvent::PageId(if index == 0 { 1 } else { 201 }))?;
            pipeline.push(PageEvent::BeginRevision)?;
            pipeline.push(PageEvent::RevisionId(1001 + index as u64))?;
            pipeline.push(PageEvent::Timestamp("2026-10-01T12:00:00Z"))?;
            pipeline.push(PageEvent::EndRevision)?;
            pipeline.push(PageEvent::EndPage)?;
            pipeline.end_member()?;
        }
        Ok(())
    }

    #[test]
    fn audited_publication_is_no_overwrite_and_removes_failed_pending_files() {
        let (root, expected, observed, digest) = fixture();
        fs::remove_file(&observed).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let pinned = inventory();
        let (published, delta) = stage_and_audit_pinned_census(
            &root,
            "ready.ids",
            &pinned,
            0,
            &expected,
            &digest,
            |pipeline| feed_two_pinned_pages(&pinned, pipeline),
        )
        .unwrap();
        assert_eq!(delta.observed_count, 2);
        assert_eq!(fs::read(&published).unwrap(), b"1\n201\n");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        assert!(matches!(
            stage_and_audit_pinned_census(
                &root,
                "ready.ids",
                &pinned,
                0,
                &expected,
                &digest,
                |pipeline| feed_two_pinned_pages(&pinned, pipeline),
            ),
            Err(VerifiedCensusStageError::Io(error))
                if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);

        fs::write(&expected, b"1\n202\n").unwrap();
        let wrong_digest = format!("{:x}", Sha256::digest(b"1\n202\n"));
        assert!(matches!(
            stage_and_audit_pinned_census(
                &root,
                "unverified.ids",
                &pinned,
                0,
                &expected,
                &wrong_digest,
                |pipeline| feed_two_pinned_pages(&pinned, pipeline),
            ),
            Err(VerifiedCensusStageError::Audit(CensusGateError::Coverage(
                StreamCoverageError::MissingOrUnexpected(_)
            )))
        ));
        assert!(!root.join("unverified.ids").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
