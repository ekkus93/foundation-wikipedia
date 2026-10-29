//! Two-pass, fail-closed staging for an unsigned verified envelope.
//! Not a publisher signature check, installer or snapshot activation.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::container::{canonical_objects, verify_container, ContainerError};
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
    validate_stage_destination(parent, name)?;

    input.seek(SeekFrom::Start(0))?;
    let manifest = verify_container(input)?;
    input.seek(SeekFrom::Start(8))?;
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    input.seek(SeekFrom::Current(i64::from(u32::from_be_bytes(length))))?;

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
    File::open(parent)?.sync_all()?;
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
    validate_stage_destination(parent, name)?;
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
            };
            verify_container(&mut tee)?
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
}

impl<R: Read> Read for SpoolingReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.source.read(buffer)?;
        self.destination.write_all(&buffer[..count])?;
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
    fn nonseekable_import_cleans_spool_after_midstream_io_error() {
        struct FailingStream {
            bytes: io::Cursor<Vec<u8>>,
            remaining: usize,
        }
        impl Read for FailingStream {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "provider disconnected"));
                }
                let n = self.bytes.read(&mut buffer[..buffer.len().min(self.remaining)])?;
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
        let mut stream = FailingStream {
            bytes: io::Cursor::new(vec![b'x'; 1024]),
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
    fn rejects_windows_device_names_before_touching_staging_parent() {
        // Invalid names fail before parent metadata or input I/O is accessed.
        let absent_parent = Path::new("missing-private-stage-parent");
        for name in [
            "CON", "con.txt", "PRN", "AUX", "NUL", "COM1", "COM9.txt", "LPT1", "lpt9.bin",
            "CONIN$", "CONOUT$",
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
