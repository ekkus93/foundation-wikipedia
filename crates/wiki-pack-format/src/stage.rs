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

/// Verify the entire input first, then stage into a newly created private
/// directory. A second complete directory digest check prevents a changed
/// input between passes from being accepted. No existing files are overwritten.
/// The caller must own the parent directory exclusively throughout the call.
pub fn stage_container<R: Read + Seek>(
    input: &mut R,
    parent: &Path,
    name: &str,
) -> Result<(Manifest, PathBuf), ContainerError> {
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('.')
        || name.ends_with('.')
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::write_container;
    use crate::manifest::{Object, Origin};
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn roundtrip_stages_only_verified_complete_content() {
        let root = std::env::temp_dir().join(format!(
            "wpack-stage-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        create_private_dir(&root).unwrap();
        let bytes = b"article";
        let manifest = Manifest {
            version: 1,
            pack_id: "science".into(),
            project: "enwiki".into(),
            snapshot: "20261001".into(),
            origin: Origin::Custom {
                definition_id: "fixture".into(),
            },
            objects: vec![Object {
                path: "articles/1.cbor".into(),
                sha256: format!("{:x}", Sha256::digest(bytes)),
                bytes: bytes.len() as u64,
            }],
        };
        let mut envelope = Vec::new();
        write_container(&mut envelope, &manifest, |_, output| output.write_all(bytes)).unwrap();
        let (read, path) = stage_container(&mut Cursor::new(&envelope), &root, "candidate").unwrap();
        assert_eq!(read, manifest);
        assert_eq!(fs::read(path.join("articles/1.cbor")).unwrap(), bytes);
        assert!(stage_container(&mut Cursor::new(&envelope), &root, "candidate").is_err());
        let mut corrupted = envelope.clone();
        *corrupted.last_mut().unwrap() ^= 1;
        assert!(stage_container(&mut Cursor::new(corrupted), &root, "bad").is_err());
        assert!(!root.join("bad").exists());
        assert!(matches!(
            stage_container(&mut Cursor::new(&envelope), &root, "../outside"),
            Err(ContainerError::UnsafeStagingPath)
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
