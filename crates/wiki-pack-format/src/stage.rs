//! Two-pass, fail-closed staging for an unsigned verified envelope.
//! Not a publisher signature check, installer or snapshot activation.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::container::{
    canonical_objects, verify_container, verify_container_with_budget, ContainerError,
};
use crate::manifest::Manifest;
use crate::objects::verify_manifest_directory;

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn validate_stage_destination(parent: &Path, name: &str) -> Result<(), ContainerError> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        || crate::manifest::unsafe_component(name)
    {
        return Err(ContainerError::UnsafeStagingPath);
    }
    for ancestor in parent.ancestors() {
        if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
            return Err(ContainerError::UnsafeStagingPath);
        }
    }
    let meta = fs::symlink_metadata(parent)?;
    if !meta.is_dir() {
        return Err(ContainerError::UnsafeStagingPath);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(ContainerError::UnsafeStagingPath);
        }
    }

    // Do not consume a seekable archive or an Android document-provider
    // stream when a destination is already occupied. create_private_dir
    // below remains the authoritative race-safe exclusive creation.
    match fs::symlink_metadata(parent.join(name)) {
        Ok(_) => return Err(ContainerError::Io(io::ErrorKind::AlreadyExists.into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(ContainerError::Io(error)),
    }

    Ok(())
}

// A failed parent-directory durability barrier must not leave a new staged
// pack behind while reporting failure to its caller.
fn finalize_staged_tree(
    parent: &Path,
    root: &Path,
    mut sync_parent: impl FnMut(&Path) -> io::Result<()>,
) -> Result<(), ContainerError> {
    if let Err(error) = sync_parent(parent) {
        // The first directory sync failed after the staged tree was created.
        // Removing it is not durable until the parent is synced again.
        if let Err(rollback) = fs::remove_dir_all(root) {
            return Err(ContainerError::Io(io::Error::other(format!(
                "staging parent sync failed: {error}; rollback removal failed: {rollback}"
            ))));
        }
        if let Err(rollback) = sync_parent(parent) {
            return Err(ContainerError::Io(io::Error::other(format!(
                "staging parent sync failed: {error}; rollback sync failed: {rollback}"
            ))));
        }
        return Err(ContainerError::Io(error));
    }
    Ok(())
}

/// Verify the entire input first, then stage into a newly created private
/// directory. A second complete directory digest check prevents a changed
/// input between passes from being accepted. No existing files are overwritten.
/// The caller must own the parent directory exclusively throughout the call.
pub fn stage_container<R: Read + Seek>(
    input: &mut R,
    parent: &Path,
    name: &str,
) -> Result<(Manifest, PathBuf), ContainerError> {
    stage_container_impl(input, parent, name, None)
}

/// Stage a seekable archive only if its entire declared envelope fits the
/// caller's byte quota. Reject an oversized manifest before allocating it
/// and an oversized declared object set before consuming payload bytes.
/// This bounds input processing, not available disk space or publisher trust.
pub fn stage_container_with_budget<R: Read + Seek>(
    input: &mut R,
    parent: &Path,
    name: &str,
    max_input_bytes: u64,
) -> Result<(Manifest, PathBuf), ContainerError> {
    stage_container_impl(input, parent, name, Some(max_input_bytes))
}

fn stage_container_impl<R: Read + Seek>(
    input: &mut R,
    parent: &Path,
    name: &str,
    max_input_bytes: Option<u64>,
) -> Result<(Manifest, PathBuf), ContainerError> {
    validate_stage_destination(parent, name)?;

    input.seek(SeekFrom::Start(0))?;
    let manifest = match max_input_bytes {
        Some(limit) => verify_container_with_budget(input, limit)?,
        None => verify_container(input)?,
    };
    // The source may change between the verification and staging passes
    // (for example, a seekable provider backed by a mutable file). Do not
    // trust the second-pass header to select a different payload offset, or
    // silently associate objects with a different pack identity. Recheck the
    // exact canonical transcript before creating any staging directory.
    let transcript = manifest.identity_transcript()?;
    input.seek(SeekFrom::Start(0))?;
    let mut header = [0u8; 12];
    input.read_exact(&mut header)?;
    if &header[..8] != b"FWPACK01"
        || u32::from_be_bytes(header[8..12].try_into().unwrap()) as usize != transcript.len()
    {
        return Err(ContainerError::InvalidTranscript);
    }
    let mut buffer = [0u8; 64 * 1024];
    for chunk in transcript.chunks(buffer.len()) {
        input.read_exact(&mut buffer[..chunk.len()])?;
        if &buffer[..chunk.len()] != chunk {
            return Err(ContainerError::InvalidTranscript);
        }
    }

    let root = parent.join(name);
    create_private_dir(&root)?;
    let result = (|| -> Result<(), ContainerError> {
        let mut buffer = [0; 64 * 1024];
        for object in canonical_objects(&manifest) {
            let mut path = root.clone();
            let components: Vec<_> = object.path.split('/').collect();
            for component in &components[..components.len() - 1] {
                path.push(component);
                match create_private_dir(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(ContainerError::Io(error)),
                }
                let meta = fs::symlink_metadata(&path)?;
                if !meta.is_dir() || meta.file_type().is_symlink() {
                    return Err(ContainerError::UnsafeStagingPath);
                }
            }
            path.push(components[components.len() - 1]);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options.open(path)?;
            let mut remaining = object.bytes;
            while remaining > 0 {
                let count = remaining.min(buffer.len() as u64) as usize;
                input.read_exact(&mut buffer[..count])?;
                output.write_all(&buffer[..count])?;
                remaining -= count as u64;
            }
            output.sync_all()?;
        }
        // The first pass rejected trailing bytes, but an input that changes
        // between passes must not smuggle an appended payload through staging.
        let mut extra = [0u8; 1];
        loop {
            match input.read(&mut extra) {
                Ok(0) => break,
                Ok(_) => return Err(ContainerError::TrailingBytes),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(ContainerError::Io(error)),
            }
        }
        verify_manifest_directory(&manifest, &root)
            .map_err(|_| ContainerError::UnsafeStagingPath)?;
        File::open(&root)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        fs::remove_dir_all(&root)?;
        return Err(error);
    }
    finalize_staged_tree(parent, &root, |path| File::open(path)?.sync_all())?;
    Ok((manifest, root))
}

/// Stage verified bytes and derive a conservative trust label from the same
/// successful verification. An unsigned official-origin claim stays unverified.
/// This does not activate or authorize installation of a staged pack.
pub fn stage_container_with_trust<R: Read + Seek>(
    input: &mut R,
    parent: &Path,
    name: &str,
) -> Result<(Manifest, PathBuf, crate::trust::TrustStatus), ContainerError> {
    let (manifest, root) = stage_container(input, parent, name)?;
    let status = crate::trust::classify(&manifest, crate::trust::IntegrityState::Passed);
    Ok((manifest, root, status))
}

/// Copy an unseekable archive stream into a private, exclusive temporary file
/// while validating its complete envelope. The verified spool is then staged
/// through the same two-pass path as a seekable archive. This supports Android
/// document-provider streams without buffering a pack in RAM.
///
/// The parent must be a caller-owned private directory with no concurrent
/// writers. The resulting tree is NOT activated or publisher-authenticated.
pub fn stage_container_stream<R: Read>(
    input: &mut R,
    parent: &Path,
    name: &str,
) -> Result<(Manifest, PathBuf), ContainerError> {
    stage_container_stream_with_budget(input, parent, name, u64::MAX)
}

/// Like stage_container_stream, but bound the private spool to a caller-selected
/// byte quota. The cap includes the envelope header, manifest and all objects.
/// No more than max_spool_bytes are written even for an untrusted stream.
/// The default API retains its existing behavior for callers without a quota.
pub fn stage_container_stream_with_budget<R: Read>(
    input: &mut R,
    parent: &Path,
    name: &str,
    max_spool_bytes: u64,
) -> Result<(Manifest, PathBuf), ContainerError> {
    validate_stage_destination(parent, name)?;
    if max_spool_bytes == 0 {
        return Err(ContainerError::ResourceBudget);
    }
    let mut spool = None;
    for _ in 0..16 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let id = NEXT_SPOOL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = parent.join(format!(".wpack-spool-{}-{nonce}-{id}", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).read(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => {
                spool = Some((path, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(ContainerError::Io(error)),
        }
    }
    let (spool_path, mut spool_file) = spool.ok_or(ContainerError::ResourceBudget)?;
    let result = (|| {
        let verified = {
            let mut tee = SpoolingReader {
                source: input,
                destination: &mut spool_file,
                remaining: max_spool_bytes,
                exceeded: false,
            };
            // Bound transcript allocation by the caller's spool quota before
            // the general envelope verifier can reserve manifest memory.
            let verification = verify_container_with_budget(&mut tee, max_spool_bytes);
            if tee.exceeded {
                return Err(ContainerError::ResourceBudget);
            }
            verification?
        };
        spool_file.sync_all()?;
        spool_file.seek(SeekFrom::Start(0))?;
        let (staged, root) = stage_container(&mut spool_file, parent, name)?;
        if staged != verified {
            fs::remove_dir_all(root)?;
            return Err(ContainerError::InvalidTranscript);
        }
        Ok((staged, root))
    })();
    drop(spool_file);
    // If spool cleanup fails after successful staging, remove the new tree
    // rather than reporting an error while leaving an apparently usable pack.
    if let Err(error) = fs::remove_file(spool_path) {
        if let Ok((_, root)) = &result {
            fs::remove_dir_all(root)?;
        }
        return Err(ContainerError::Io(error));
    }
    result
}

/// A tee that does not require Seek from the source and persists only bytes
/// actually consumed by the bounded envelope verifier.
struct SpoolingReader<'a, R> {
    source: &'a mut R,
    destination: &'a mut File,
    remaining: u64,
    exceeded: bool,
}

impl<R: Read> Read for SpoolingReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let mut probe = [0u8; 1];
            return match self.source.read(&mut probe)? {
                0 => Ok(0),
                _ => {
                    self.exceeded = true;
                    Err(io::Error::other("pack stream spool quota exceeded"))
                }
            };
        }
        let allowed = self.remaining.min(buffer.len() as u64) as usize;
        let count = self.source.read(&mut buffer[..allowed])?;
        self.destination.write_all(&buffer[..count])?;
        self.remaining -= count as u64;
        Ok(count)
    }
}

static NEXT_SPOOL_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Classify a successfully staged unseekable pack conservatively: even an
/// unsigned official-origin claim never becomes Verified Publisher.
pub fn stage_container_stream_with_trust<R: Read>(
    input: &mut R,
    parent: &Path,
    name: &str,
) -> Result<(Manifest, PathBuf, crate::trust::TrustStatus), ContainerError> {
    let (manifest, root) = stage_container_stream(input, parent, name)?;
    let status = crate::trust::classify(&manifest, crate::trust::IntegrityState::Passed);
    Ok((manifest, root, status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Object, Origin, FORMAT_VERSION};
    use crate::trust::TrustStatus;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct ReadOnly<R>(R);

    impl<R: Read> Read for ReadOnly<R> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(buffer)
        }
    }

    /// A valid replacement envelope with identical objects must not be
    /// accepted under the metadata authenticated during the first pass.
    #[test]
    fn seekable_staging_rejects_manifest_swap_between_verification_and_copy() {
        struct SwappingInput {
            current: io::Cursor<Vec<u8>>,
            replacement: Vec<u8>,
            seeks: usize,
        }
        impl Read for SwappingInput {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.current.read(bytes)
            }
        }
        impl Seek for SwappingInput {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                if self.seeks == 1 {
                    self.current = io::Cursor::new(std::mem::take(&mut self.replacement));
                }
                self.seeks += 1;
                self.current.seek(position)
            }
        }

        let payload = b"revision-matched diagram";
        let mut manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "alpha".into(),
            project: "enwiki".into(),
            snapshot: "20261030".into(),
            origin: Origin::Custom {
                definition_id: "topic".into(),
            },
            objects: vec![Object {
                path: "media/diagram.svg".into(),
                sha256: crate::sha256_hex(payload),
                bytes: payload.len() as u64,
            }],
        };
        let mut original = Vec::new();
        crate::container::write_container(&mut original, &manifest, |_, output| {
            output.write_all(payload)
        })
        .unwrap();
        manifest.pack_id = "omega".into();
        let mut replacement = Vec::new();
        crate::container::write_container(&mut replacement, &manifest, |_, output| {
            output.write_all(payload)
        })
        .unwrap();
        assert_eq!(original.len(), replacement.len());

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stage-swap-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let mut input = SwappingInput {
            current: io::Cursor::new(original),
            replacement,
            seeks: 0,
        };
        assert!(matches!(
            stage_container(&mut input, &parent, "swapped"),
            Err(ContainerError::InvalidTranscript)
        ));
        assert!(!parent.join("swapped").exists());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn seekable_stage_budget_rejects_oversized_declarations_before_copy() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stage-budget-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let payload = b"offline revision figure";
        let manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "bounded".into(),
            project: "enwiki".into(),
            snapshot: "20261030".into(),
            origin: Origin::Custom {
                definition_id: "topic".into(),
            },
            objects: vec![Object {
                path: "media/figure.svg".into(),
                sha256: crate::sha256_hex(payload),
                bytes: payload.len() as u64,
            }],
        };
        let mut encoded = Vec::new();
        crate::container::write_container(&mut encoded, &manifest, |_, out| out.write_all(payload))
            .unwrap();
        let exact = encoded.len() as u64;
        let transcript_end = 12 + manifest.identity_transcript().unwrap().len() as u64;

        let mut too_small = io::Cursor::new(encoded.clone());
        assert!(matches!(
            stage_container_with_budget(&mut too_small, &parent, "rejected", exact - 1),
            Err(ContainerError::ResourceBudget)
        ));
        assert_eq!(too_small.position(), transcript_end);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);

        let mut accepted = io::Cursor::new(encoded);
        let (verified, root) =
            stage_container_with_budget(&mut accepted, &parent, "accepted", exact).unwrap();
        assert_eq!(verified, manifest);
        assert_eq!(fs::read(root.join("media/figure.svg")).unwrap(), payload);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);

        let mut header = b"FWPACK01".to_vec();
        header.extend_from_slice(&(crate::container::MAX_MANIFEST_BYTES as u32).to_be_bytes());
        let mut forged = io::Cursor::new(header.clone());
        assert!(matches!(
            stage_container_with_budget(&mut forged, &parent, "forged", 32),
            Err(ContainerError::ResourceBudget)
        ));
        assert_eq!(forged.position(), 12);

        let mut short = io::Cursor::new(header);
        assert!(matches!(
            stage_container_with_budget(&mut short, &parent, "short", 11),
            Err(ContainerError::ResourceBudget)
        ));
        assert_eq!(short.position(), 0);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn nonseekable_document_stream_stages_verified_objects_without_spool_residue() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stream-stage-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let payload = b"offline figure";
        let manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "figures".into(),
            project: "enwiki".into(),
            snapshot: "20261029".into(),
            origin: Origin::Custom {
                definition_id: "topic".into(),
            },
            objects: vec![Object {
                path: "media/figure.svg".into(),
                sha256: crate::sha256_hex(payload),
                bytes: payload.len() as u64,
            }],
        };
        let mut encoded = Vec::new();
        crate::container::write_container(&mut encoded, &manifest, |_, out| out.write_all(payload))
            .unwrap();

        let mut stream = ReadOnly(io::Cursor::new(encoded.clone()));
        let (staged, root, status) =
            stage_container_stream_with_trust(&mut stream, &parent, "imported").unwrap();
        assert_eq!(staged, manifest);
        assert_eq!(status, TrustStatus::CustomIntegrityVerified);
        assert_eq!(fs::read(root.join("media/figure.svg")).unwrap(), payload);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);

        // Corrupt bytes and appended payloads must not leave a partial tree
        // or an archive spool on disk.
        for (name, mut invalid) in [("corrupt", encoded.clone()), ("trailing", encoded)] {
            if name == "corrupt" {
                *invalid.last_mut().unwrap() ^= 1;
            } else {
                invalid.push(0);
            }
            let mut stream = ReadOnly(io::Cursor::new(invalid));
            assert!(stage_container_stream(&mut stream, &parent, name).is_err());
            assert!(!parent.join(name).exists());
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        }
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn nonseekable_spool_quota_accepts_exact_size_and_cleans_up_on_overflow() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stream-quota-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let payload = b"offline diagram";
        let manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "bounded".into(),
            project: "enwiki".into(),
            snapshot: "20261029".into(),
            origin: Origin::Custom {
                definition_id: "topic".into(),
            },
            objects: vec![Object {
                path: "media/diagram.svg".into(),
                sha256: crate::sha256_hex(payload),
                bytes: payload.len() as u64,
            }],
        };
        let mut encoded = Vec::new();
        crate::container::write_container(&mut encoded, &manifest, |_, out| out.write_all(payload))
            .unwrap();
        let quota = encoded.len() as u64;
        let mut exact = ReadOnly(io::Cursor::new(encoded.clone()));
        let (_, root) =
            stage_container_stream_with_budget(&mut exact, &parent, "exact", quota).unwrap();
        assert_eq!(fs::read(root.join("media/diagram.svg")).unwrap(), payload);

        let mut overflow = ReadOnly(io::Cursor::new(encoded));
        assert!(matches!(
            stage_container_stream_with_budget(&mut overflow, &parent, "overflow", quota - 1),
            Err(ContainerError::ResourceBudget)
        ));
        assert!(!parent.join("overflow").exists());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn nonseekable_spool_preflights_manifest_allocation_against_quota() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stream-header-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let mut header = b"FWPACK01".to_vec();
        header.extend_from_slice(&(crate::container::MAX_MANIFEST_BYTES as u32).to_be_bytes());

        // A forged large transcript with a tiny spool quota must be rejected
        // after the fixed header, before reading or allocating its body.
        let mut input = ReadOnly(io::Cursor::new(header.clone()));
        assert!(matches!(
            stage_container_stream_with_budget(&mut input, &parent, "oversized", 32),
            Err(ContainerError::ResourceBudget)
        ));
        assert_eq!(input.0.position(), 12);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);

        // A quota smaller than the fixed header must not read the source.
        let mut input = ReadOnly(io::Cursor::new(header));
        assert!(matches!(
            stage_container_stream_with_budget(&mut input, &parent, "short", 11),
            Err(ContainerError::ResourceBudget)
        ));
        assert_eq!(input.0.position(), 0);
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn nonseekable_import_cleans_spool_after_midstream_io_error() {
        struct FailingStream {
            bytes: io::Cursor<Vec<u8>>,
            remaining: usize,
        }
        impl Read for FailingStream {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "provider disconnected",
                    ));
                }
                let limit = buffer.len().min(self.remaining);
                let n = self.bytes.read(&mut buffer[..limit])?;
                self.remaining -= n;
                Ok(n)
            }
        }

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stream-error-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let mut encoded = b"FWPACK01".to_vec();
        encoded.extend_from_slice(&50u32.to_be_bytes());
        encoded.extend_from_slice(&[0u8; 1024]);
        let mut stream = FailingStream {
            bytes: io::Cursor::new(encoded),
            remaining: 17,
        };
        assert!(matches!(
            stage_container_stream(&mut stream, &parent, "interrupted"),
            Err(ContainerError::Io(_))
        ));
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn nonseekable_import_rejects_unsafe_name_before_reading_input() {
        struct PanicOnRead;
        impl Read for PanicOnRead {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("invalid stage name must fail before reading input")
            }
        }
        assert!(matches!(
            stage_container_stream(&mut PanicOnRead, Path::new("nonexistent-parent"), "CON.txt"),
            Err(ContainerError::UnsafeStagingPath)
        ));
    }

    #[test]
    fn occupied_destination_rejects_seekable_and_nonseekable_input_without_reading() {
        struct PanicInput;
        impl Read for PanicInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("occupied destination must not consume input");
            }
        }
        impl Seek for PanicInput {
            fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
                panic!("occupied destination must not seek input");
            }
        }

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent = std::env::temp_dir().join(format!(
            "wiki-stage-occupied-{}-{stamp}",
            std::process::id()
        ));
        create_private_dir(&parent).unwrap();
        let existing = parent.join("installed");
        create_private_dir(&existing).unwrap();
        fs::write(existing.join("sentinel"), b"keep").unwrap();

        for stream in [false, true] {
            let result = if stream {
                stage_container_stream(&mut PanicInput, &parent, "installed")
            } else {
                stage_container(&mut PanicInput, &parent, "installed")
            };
            assert!(matches!(
                result,
                Err(ContainerError::Io(ref error))
                    if error.kind() == io::ErrorKind::AlreadyExists
            ));
            assert_eq!(fs::read(existing.join("sentinel")).unwrap(), b"keep");
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
        }
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn failed_parent_durability_sync_rolls_back_new_staging_tree() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-stage-sync-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let root = parent.join("new-stage");
        create_private_dir(&root).unwrap();
        fs::write(root.join("object"), b"unpublished").unwrap();
        let mut sync_calls = 0;
        let result = finalize_staged_tree(&parent, &root, |_| {
            sync_calls += 1;
            if sync_calls == 1 {
                Err(io::Error::other("simulated directory sync failure"))
            } else {
                assert!(!root.exists(), "rollback must remove the staged tree first");
                Ok(())
            }
        });
        assert_eq!(sync_calls, 2, "publish and rollback must each sync the parent");
        match result {
            Err(ContainerError::Io(error)) => {
                assert!(error.to_string().contains("simulated directory sync failure"));
            }
            other => panic!("unexpected staging outcome: {other:?}"),
        }
        assert!(!root.exists());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn failed_rollback_parent_sync_is_reported_separately() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent = std::env::temp_dir().join(format!(
            "wiki-stage-rollback-sync-{}-{stamp}",
            std::process::id()
        ));
        create_private_dir(&parent).unwrap();
        let root = parent.join("new-stage");
        create_private_dir(&root).unwrap();
        fs::write(root.join("object"), b"unpublished").unwrap();

        let mut sync_calls = 0;
        let result = finalize_staged_tree(&parent, &root, |_| {
            sync_calls += 1;
            if sync_calls == 1 {
                Err(io::Error::other("publish sync failed"))
            } else {
                Err(io::Error::other("rollback sync failed"))
            }
        });
        assert_eq!(sync_calls, 2);
        match result {
            Err(ContainerError::Io(error)) => {
                let message = error.to_string();
                assert!(message.contains("publish sync failed"), "{message}");
                assert!(message.contains("rollback sync failed"), "{message}");
            }
            other => panic!("unexpected staging outcome: {other:?}"),
        }
        assert!(!root.exists());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn rejects_windows_device_names_before_touching_staging_parent() {
        // Invalid names fail before parent metadata or input I/O is accessed.
        let absent_parent = Path::new("missing-private-stage-parent");
        for name in [
            "CON",
            "con.txt",
            "PRN",
            "AUX",
            "NUL",
            "COM1",
            "COM9.txt",
            "LPT1",
            "lpt9.bin",
            "CONIN$",
            "CONOUT$",
            "CON .txt",
            "NUL  .bin",
            "COM1 .svg",
            "LPT9 .log",
            "CONOUT$ .txt",
        ] {
            let mut empty = io::Cursor::new(Vec::<u8>::new());
            assert!(matches!(
                stage_container(&mut empty, absent_parent, name),
                Err(ContainerError::UnsafeStagingPath)
            ));
        }
    }

    #[test]
    fn staged_custom_integrity_does_not_imply_official_publisher_trust() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent =
            std::env::temp_dir().join(format!("wiki-trust-stage-{}-{stamp}", std::process::id()));
        create_private_dir(&parent).unwrap();
        let payload = b"verified article";
        let mut manifest = Manifest {
            version: FORMAT_VERSION,
            pack_id: "physics".into(),
            project: "enwiki".into(),
            snapshot: "20261013".into(),
            origin: Origin::Custom {
                definition_id: "physics".into(),
            },
            objects: vec![Object {
                path: "articles/42.cbor".into(),
                sha256: crate::sha256_hex(payload),
                bytes: payload.len() as u64,
            }],
        };
        for (name, expected) in [
            ("custom", TrustStatus::CustomIntegrityVerified),
            ("claimed-official", TrustStatus::NotYetChecked),
        ] {
            if name == "claimed-official" {
                manifest.origin = Origin::Official {
                    publisher: "unverified".into(),
                };
            }
            let mut encoded = Vec::new();
            crate::container::write_container(&mut encoded, &manifest, |_, out| {
                out.write_all(payload)
            })
            .unwrap();
            let (staged, root, status) =
                stage_container_with_trust(&mut std::io::Cursor::new(encoded), &parent, name)
                    .unwrap();
            assert_eq!(staged, manifest);
            assert_eq!(status, expected);
            assert_eq!(fs::read(root.join("articles/42.cbor")).unwrap(), payload);
        }
        fs::remove_dir_all(parent).unwrap();
    }
}
