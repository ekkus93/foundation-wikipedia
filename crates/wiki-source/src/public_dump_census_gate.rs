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
    // A regular leaf file can still be reached through a symlinked parent.
    // Census inputs must use an entirely non-symlinked path before opening.
    for ancestor in path.ancestors() {
        if fs::symlink_metadata(ancestor)
            .map_err(CensusGateError::Io)?
            .file_type()
            .is_symlink()
        {
            return Err(CensusGateError::UnsafeInput);
        }
    }
    let link = fs::symlink_metadata(path).map_err(CensusGateError::Io)?;
    if !link.file_type().is_file() || link.len() == 0 || link.len() > MAX_CENSUS_BYTES {
        return Err(CensusGateError::UnsafeInput);
    }
    let file = File::open(path).map_err(CensusGateError::Io)?;
    let opened = file.metadata().map_err(CensusGateError::Io)?;
    if !opened.is_file()
        || !same_identity(&link, &opened)
        || opened.len() != link.len()
        || opened.len() == 0
        || opened.len() > MAX_CENSUS_BYTES
    {
        return Err(CensusGateError::UnsafeInput);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Both the expected and observed census must be isolated from any
        // additional hard-link name; otherwise a second pathname could mutate
        // the same inode even when these two paths are distinct.
        if link.nlink() != 1 || opened.nlink() != 1 {
            return Err(CensusGateError::UnsafeInput);
        }
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

// A file may grow after its initial metadata check. Bound bytes read during
// both hash passes rather than trusting a potentially stale file length.
fn sha256_bounded<R: Read>(reader: &mut R, max_bytes: u64) -> Result<String, CensusGateError> {
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    let mut interrupted = 0u8;
    loop {
        let size = match reader.read(&mut buffer) {
            Ok(size) => {
                interrupted = 0;
                size
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted && interrupted < 8 => {
                interrupted += 1;
                continue;
            }
            Err(error) => return Err(CensusGateError::Io(error)),
        };
        if size == 0 {
            break;
        }
        total = total
            .checked_add(size as u64)
            .ok_or(CensusGateError::UnsafeInput)?;
        if total > max_bytes {
            return Err(CensusGateError::UnsafeInput);
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn sha256_file(file: &mut File) -> Result<String, CensusGateError> {
    file.seek(SeekFrom::Start(0)).map_err(CensusGateError::Io)?;
    let digest = sha256_bounded(file, MAX_CENSUS_BYTES)?;
    file.seek(SeekFrom::Start(0)).map_err(CensusGateError::Io)?;
    Ok(digest)
}

/** Reject an invalid or modified independently pinned expected census before
 * processing potentially terabytes of dump shards. The final coverage gate
 * reopens and rehashes this file after decoding: preflight is not provenance
 * authentication and never replaces the post-ingestion audit. */
fn preflight_expected_census(path: &Path, sha256: &str) -> Result<(), CensusGateError> {
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CensusGateError::InvalidDigest);
    }
    let (mut file, before) = open_regular(path)?;
    if !sha256_file(&mut file)?.eq_ignore_ascii_case(sha256) {
        return Err(CensusGateError::ExpectedDigestMismatch);
    }
    if !unchanged(&before, &file.metadata().map_err(CensusGateError::Io)?) {
        return Err(CensusGateError::ChangedDuringAudit);
    }
    Ok(())
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
    // Reject an occupied destination before potentially decompressing an
    // entire dump generation. The final hard-link publication below remains
    // the authoritative no-overwrite check against concurrent writers.
    let destination = root.join(filename);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(VerifiedCensusStageError::Io(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "audited census destination already exists",
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(VerifiedCensusStageError::Io(error)),
    }
    // A bad independent pin is known before the first expensive XML shard
    // read. The final audit still verifies the expected file after staging.
    preflight_expected_census(expected_path, expected_sha256)
        .map_err(VerifiedCensusStageError::Audit)?;
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

/// Decode every inventory-pinned native bzip2/XML shard and publish the
/// namespace census only after an independently supplied, digest-pinned
/// expected census agrees exactly. Unlike the semantic-event variant, this
/// entry point does not trust a caller-provided sequence of page events.
///
/// The caller must authenticate the inventory and expected census separately,
/// including publisher identity, snapshot cutoff and namespace semantics.
/// Matching hashes or page IDs alone do not establish that independence.
#[derive(Clone, Copy)]
pub struct NativeCensusAudit<'a> {
    pub expected_path: &'a Path,
    pub expected_sha256: &'a str,
    pub max_compressed_bytes: u64,
    pub max_decoded_bytes: u64,
}

/// The independently pinned expected census and resource limits travel as
/// one configuration, keeping the native audit boundary explicit.
pub fn stage_and_audit_native_bzip2_census<R, F>(
    root: &Path,
    filename: &str,
    inventory: &PinnedInventory,
    namespace: i32,
    audit: NativeCensusAudit<'_>,
    mut open_member: F,
) -> Result<(std::path::PathBuf, PageCoverageDelta), VerifiedCensusStageError>
where
    R: Read,
    F: FnMut(&str) -> io::Result<R>,
{
    use crate::public_dump_census_ingest::{NativeCensusError, PinnedCensusError};

    stage_and_audit_pinned_census(
        root,
        filename,
        inventory,
        namespace,
        audit.expected_path,
        audit.expected_sha256,
        |pipeline| {
            for member in inventory.members() {
                let input =
                    open_member(&member.relative_path).map_err(PinnedCensusError::Source)?;
                pipeline
                    .decode_native_bzip2_member(
                        input,
                        audit.max_compressed_bytes,
                        audit.max_decoded_bytes,
                    )
                    .map_err(|error| match error {
                        NativeCensusError::Census(error) => error,
                        NativeCensusError::Decode(error) => PinnedCensusError::NativeDecode(error),
                    })?;
            }
            Ok(())
        },
    )
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

    struct InterruptingCensusInput<'a> {
        bytes: &'a [u8],
        offset: usize,
        interrupted: u8,
        reset_once: bool,
    }

    impl Read for InterruptingCensusInput<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.interrupted > 0 {
                self.interrupted -= 1;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.offset == self.bytes.len() {
                return Ok(0);
            }
            let count = (self.bytes.len() - self.offset).min(out.len()).min(2);
            out[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
            self.offset += count;
            if !self.reset_once {
                self.reset_once = true;
                self.interrupted = 8;
            }
            Ok(count)
        }
    }

    #[test]
    fn bounded_census_hash_retries_interruptions_and_fails_persistently() {
        let bytes = b"1\\n201\\n";
        let mut transient = InterruptingCensusInput {
            bytes,
            offset: 0,
            interrupted: 8,
            reset_once: false,
        };
        assert_eq!(
            sha256_bounded(&mut transient, 1024).unwrap(),
            format!("{:x}", Sha256::digest(bytes))
        );
        let mut persistent = InterruptingCensusInput {
            bytes,
            offset: 0,
            interrupted: 9,
            reset_once: true,
        };
        assert!(matches!(
            sha256_bounded(&mut persistent, 1024),
            Err(CensusGateError::Io(error)) if error.kind() == io::ErrorKind::Interrupted
        ));
        assert_eq!(persistent.offset, 0);
    }

    #[test]
    fn native_bzip2_census_is_published_only_after_independent_expected_audit() {
        use std::io::{Cursor, Write};

        let root = std::env::temp_dir().join(format!(
            "wiki-native-audited-census-{}-{}",
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
            .map(|source| {
                let mut encoder =
                    bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
                encoder.write_all(source.as_bytes()).unwrap();
                encoder.finish().unwrap()
            })
            .collect();
        let names = [
            "enwiki-2026-10-01-p1p100.xml.bz2",
            "enwiki-2026-10-01-p201p300.xml.bz2",
        ];
        let sums = format!(
            "{:x}  {}\n{:x}  {}\n",
            Sha256::digest(&compressed[0]),
            names[0],
            Sha256::digest(&compressed[1]),
            names[1]
        );
        let pinned = parse_pinned_inventory(
            sums.as_bytes(),
            "enwiki",
            "2026-10-01",
            &format!("{:x}", Sha256::digest(sums.as_bytes())),
        )
        .unwrap();
        let expected = root.join("expected.ids");
        fs::write(&expected, b"1\n201\n").unwrap();
        let expected_digest = format!("{:x}", Sha256::digest(b"1\n201\n"));
        let audit = NativeCensusAudit {
            expected_path: &expected,
            expected_sha256: &expected_digest,
            max_compressed_bytes: 4096,
            max_decoded_bytes: 4096,
        };
        let open = |name: &str| {
            let index = names
                .iter()
                .position(|candidate| *candidate == name)
                .unwrap();
            Ok::<_, io::Error>(Cursor::new(compressed[index].as_slice()))
        };
        let (ready, delta) =
            stage_and_audit_native_bzip2_census(&root, "ready.ids", &pinned, 0, audit, open)
                .unwrap();
        assert_eq!(delta.expected_count, 2);
        assert_eq!(delta.observed_count, 2);
        assert_eq!(fs::read(&ready).unwrap(), b"1\n201\n");

        // An existing published census is rejected before opening or
        // decompressing even the first source shard.
        let mut opens = 0usize;
        assert!(matches!(
            stage_and_audit_native_bzip2_census(
                &root,
                "ready.ids",
                &pinned,
                0,
                audit,
                |name| {
                    opens += 1;
                    open(name)
                },
            ),
            Err(VerifiedCensusStageError::Io(error))
                if error.kind() == io::ErrorKind::AlreadyExists
        ));
        assert_eq!(opens, 0, "occupied outputs must fail before expensive I/O");
        #[cfg(unix)]
        {
            // A dangling symlink is still an occupied destination.
            use std::os::unix::fs::symlink;
            let alias = root.join("dangling.ids");
            symlink(root.join("missing-target.ids"), &alias).unwrap();
            assert!(matches!(
                stage_and_audit_native_bzip2_census(
                    &root,
                    "dangling.ids",
                    &pinned,
                    0,
                    audit,
                    |_| -> io::Result<std::io::Cursor<&[u8]>> {
                        panic!("must reject dangling symlink before opening a shard")
                    },
                ),
                Err(VerifiedCensusStageError::Io(error))
                    if error.kind() == io::ErrorKind::AlreadyExists
            ));
            fs::remove_file(alias).unwrap();
        }

        // Correctly pinned but different expected IDs must not be published.
        fs::write(&expected, b"1\n202\n").unwrap();
        let different_digest = format!("{:x}", Sha256::digest(b"1\n202\n"));
        assert!(matches!(
            stage_and_audit_native_bzip2_census(
                &root,
                "mismatch.ids",
                &pinned,
                0,
                NativeCensusAudit {
                    expected_sha256: &different_digest,
                    ..audit
                },
                open,
            ),
            Err(VerifiedCensusStageError::Audit(CensusGateError::Coverage(
                StreamCoverageError::MissingOrUnexpected(_)
            )))
        ));

        // A forged second-member checksum fails before expected-census audit.
        let bad_sums = format!(
            "{:x}  {}\n{}  {}\n",
            Sha256::digest(&compressed[0]),
            names[0],
            "0".repeat(64),
            names[1]
        );
        let bad_pinned = parse_pinned_inventory(
            bad_sums.as_bytes(),
            "enwiki",
            "2026-10-01",
            &format!("{:x}", Sha256::digest(bad_sums.as_bytes())),
        )
        .unwrap();
        assert!(matches!(
            stage_and_audit_native_bzip2_census(
                &root,
                "corrupt.ids",
                &bad_pinned,
                0,
                NativeCensusAudit {
                    expected_sha256: &different_digest,
                    ..audit
                },
                open,
            ),
            Err(VerifiedCensusStageError::Stage(
                crate::public_dump_census_ingest::CensusStageError::Census(
                    crate::public_dump_census_ingest::PinnedCensusError::NativeDecode(
                        crate::public_dump_checked_read::CheckedMemberError::DigestMismatch
                    )
                )
            ))
        ));
        assert!(!root.join("mismatch.ids").exists());
        assert!(!root.join("corrupt.ids").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        assert_eq!(fs::read(&ready).unwrap(), b"1\n201\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_expected_pin_fails_before_any_native_member_is_opened() {
        let (root, expected, observed, digest) = fixture();
        fs::remove_file(&observed).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let pinned = inventory();
        let mut opened = 0usize;
        for (filename, bad_digest) in [
            ("bad-digest.ids", "not-a-sha256".to_owned()),
            ("mismatch.ids", "0".repeat(64)),
        ] {
            let result = stage_and_audit_native_bzip2_census(
                &root,
                filename,
                &pinned,
                0,
                NativeCensusAudit {
                    expected_path: &expected,
                    expected_sha256: &bad_digest,
                    max_compressed_bytes: 4096,
                    max_decoded_bytes: 4096,
                },
                |_| -> io::Result<std::io::Cursor<&[u8]>> {
                    opened += 1;
                    panic!("invalid expected pin must fail before source decoding")
                },
            );
            assert!(matches!(
                result,
                Err(VerifiedCensusStageError::Audit(CensusGateError::InvalidDigest))
                    | Err(VerifiedCensusStageError::Audit(
                        CensusGateError::ExpectedDigestMismatch
                    ))
            ));
            assert!(!root.join(filename).exists());
        }
        assert_eq!(opened, 0);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        // A correct expected pin still reaches the native member opener;
        // preflight is not a replacement for complete source verification.
        assert!(preflight_expected_census(&expected, &digest).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_hashing_rejects_growth_past_the_declared_budget() {
        let bytes = b"1\n201\n";
        let mut oversized = io::Cursor::new(bytes);
        assert!(matches!(
            sha256_bounded(&mut oversized, 4),
            Err(CensusGateError::UnsafeInput)
        ));
        let mut valid = io::Cursor::new(bytes);
        assert_eq!(
            sha256_bounded(&mut valid, bytes.len() as u64).unwrap(),
            format!("{:x}", Sha256::digest(bytes))
        );
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
        // Hard-linked files are unsafe independently of whether both audit
        // paths refer to the same inode.
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &linked),
            Err(CensusGateError::UnsafeInput)
        ));
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed),
            Err(CensusGateError::UnsafeInput)
        ));
        fs::remove_file(&linked).unwrap();
        assert!(audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed).is_ok());

        let third_party_alias = root.join("observed-alias.ids");
        fs::hard_link(&observed, &third_party_alias).unwrap();
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed),
            Err(CensusGateError::UnsafeInput)
        ));
        fs::remove_file(&third_party_alias).unwrap();
        assert!(audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed).is_ok());

        let parent_alias = root.with_extension("parent-alias");
        symlink(root.parent().unwrap(), &parent_alias).unwrap();
        let alias_root = parent_alias.join(root.file_name().unwrap());
        let alias_expected = alias_root.join("expected.ids");
        let alias_observed = alias_root.join("observed.ids");
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &alias_expected, &digest, &observed),
            Err(CensusGateError::UnsafeInput)
        ));
        assert!(matches!(
            audit_pinned_census_files(&inventory(), 0, &expected, &digest, &alias_observed),
            Err(CensusGateError::UnsafeInput)
        ));
        fs::remove_file(parent_alias).unwrap();
        assert!(audit_pinned_census_files(&inventory(), 0, &expected, &digest, &observed).is_ok());
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
