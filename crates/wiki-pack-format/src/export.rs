//! Build a provisional custom-pack manifest from a private, immutable object
//! tree, then stream the object bytes into the unsigned envelope format.
//!
//! This is NOT an official publisher pack, a license/media completeness gate,
//! a safe archive installer, or an atomic output publisher.

use crate::container::{write_container, ContainerError};
use std::collections::BTreeSet;
use crate::manifest::{
    Manifest, ManifestError, Object, Origin, FORMAT_VERSION, MAX_DECLARED_BYTES, MAX_OBJECT_BYTES,
    MAX_PACK_OBJECTS,
};
use crate::objects::{verify_manifest_directory, ObjectVerifyError};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

#[derive(Debug)]
pub enum ExportError {
    Io(io::Error),
    Manifest(ManifestError),
    InvalidTree,
    ResourceBudget,
    Verify(ObjectVerifyError),
    Envelope(ContainerError),
}

impl From<io::Error> for ExportError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ManifestError> for ExportError {
    fn from(value: ManifestError) -> Self {
        Self::Manifest(value)
    }
}

impl From<ContainerError> for ExportError {
    fn from(value: ContainerError) -> Self {
        Self::Envelope(value)
    }
}

fn private_root(root: &Path) -> Result<(), ExportError> {
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(ancestor)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(ExportError::InvalidTree);
        }
    }
    Ok(())
}

fn hash_object(path: &Path) -> Result<(u64, String), ExportError> {
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(ExportError::InvalidTree);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.nlink() != 1 {
            return Err(ExportError::InvalidTree);
        }
    }
    if before.len() == 0 || before.len() > MAX_OBJECT_BYTES {
        return Err(ExportError::ResourceBudget);
    }
    let mut file = File::open(path)?;
    let opened = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.ino() != opened.ino() || before.dev() != opened.dev() || opened.nlink() != 1 {
            return Err(ExportError::InvalidTree);
        }
    }
    if opened.len() != before.len() {
        return Err(ExportError::InvalidTree);
    }
    let mut hash = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ExportError::Io(error)),
        };
        if n == 0 {
            break;
        }
        total = total
            .checked_add(n as u64)
            .ok_or(ExportError::ResourceBudget)?;
        if total > before.len() {
            return Err(ExportError::InvalidTree);
        }
        hash.update(&buffer[..n]);
    }
    if total != before.len() || file.metadata()?.len() != total {
        return Err(ExportError::InvalidTree);
    }
    Ok((total, format!("{:x}", hash.finalize())))
}

/// Inventory a directory of canonical source objects without consuming the
/// entire payloads in memory. Reject links, non-regular entries and unsafe
/// manifest paths. The tree must be immutable and exclusively controlled by
/// the caller for the entire inventory and export operation.
pub fn custom_manifest_from_tree(
    root: &Path,
    pack_id: &str,
    project: &str,
    snapshot: &str,
    definition_id: &str,
) -> Result<Manifest, ExportError> {
    private_root(root)?;
    let mut objects = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    let mut declared = 0u64;
    let mut directories = 0usize;
    while let Some((dir, prefix)) = stack.pop() {
        directories += 1;
        if directories > MAX_PACK_OBJECTS {
            return Err(ExportError::ResourceBudget);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(ExportError::InvalidTree)?;
            let relative = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(ExportError::InvalidTree);
            }
            if meta.is_dir() {
                stack.push((path, relative));
            } else if meta.is_file() {
                if objects.len() >= MAX_PACK_OBJECTS {
                    return Err(ExportError::ResourceBudget);
                }
                let (bytes, sha256) = hash_object(&path)?;
                declared = declared
                    .checked_add(bytes)
                    .ok_or(ExportError::ResourceBudget)?;
                if declared > MAX_DECLARED_BYTES {
                    return Err(ExportError::ResourceBudget);
                }
                objects.push(Object {
                    path: relative,
                    sha256,
                    bytes,
                });
            } else {
                return Err(ExportError::InvalidTree);
            }
        }
    }
    let manifest = Manifest {
        version: FORMAT_VERSION,
        pack_id: pack_id.to_owned(),
        project: project.to_owned(),
        snapshot: snapshot.to_owned(),
        origin: Origin::Custom {
            definition_id: definition_id.to_owned(),
        },
        objects,
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Reject any new, missing, or unlisted object between inventory and
/// publication. Checking only declared hashes cannot detect new files.
/// The tree must still be exclusively owned to defeat rename races.
fn verify_object_membership(root: &Path, manifest: &Manifest) -> Result<(), ExportError> {
    manifest.validate()?;
    let mut expected: BTreeSet<String> = manifest
        .objects
        .iter()
        .map(|object| object.path.clone())
        .collect();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    let mut directories = 0usize;
    while let Some((dir, prefix)) = stack.pop() {
        directories += 1;
        if directories > MAX_PACK_OBJECTS {
            return Err(ExportError::ResourceBudget);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().ok_or(ExportError::InvalidTree)?;
            let relative = if prefix.is_empty() {
                filename.to_owned()
            } else {
                format!("{prefix}/{filename}")
            };
            let path = entry.path();
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() {
                return Err(ExportError::InvalidTree);
            }
            if meta.is_dir() {
                stack.push((path, relative));
            } else if meta.is_file() {
                if !expected.remove(&relative) {
                    return Err(ExportError::InvalidTree);
                }
            } else {
                return Err(ExportError::InvalidTree);
            }
        }
    }
    if !expected.is_empty() {
        return Err(ExportError::InvalidTree);
    }
    Ok(())
}

/// Verify every inventoried object again, then stream the canonical envelope.
/// The output writer may contain partial bytes on error; never publish before
/// this returns Ok AND the caller re-verifies and fsyncs the complete output.
pub fn write_custom_tree<W: Write>(
    root: &Path,
    manifest: &Manifest,
    output: &mut W,
) -> Result<(), ExportError> {
    private_root(root)?;
    if !matches!(manifest.origin, Origin::Custom { .. }) {
        return Err(ExportError::InvalidTree);
    }
    verify_object_membership(root, manifest)?;
    verify_manifest_directory(manifest, root).map_err(ExportError::Verify)?;
    write_container(output, manifest, |object, writer| {
        let path = root.join(&object.path);
        let mut source = File::open(path)?;
        io::copy(&mut source, writer)?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::verify_container;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("wiki-export-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn custom_tree_exports_canonical_verified_payloads() {
        let root = root();
        fs::create_dir(root.join("media")).unwrap();
        fs::write(root.join("article.cbor"), b"verified article").unwrap();
        fs::write(root.join("media/diagram.svg"), b"<svg/>").unwrap();
        let manifest =
            custom_manifest_from_tree(&root, "math", "enwiki", "20261001", "math").unwrap();
        assert_eq!(manifest.objects.len(), 2);
        assert!(matches!(manifest.origin, Origin::Custom { .. }));
        let mut output = Vec::new();
        write_custom_tree(&root, &manifest, &mut output).unwrap();
        let verified = verify_container(&mut output.as_slice()).unwrap();
        assert_eq!(verified, manifest);
        manifest
            .objects
            .iter()
            .for_each(|object| assert_eq!(object.sha256.len(), 64));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_changed_sources_and_empty_manifest() {
        let root = root();
        assert!(matches!(
            custom_manifest_from_tree(&root, "math", "enwiki", "20261001", "math"),
            Err(ExportError::Manifest(ManifestError::MissingMetadata))
        ));
        fs::write(root.join("article.cbor"), b"original").unwrap();
        let manifest =
            custom_manifest_from_tree(&root, "math", "enwiki", "20261001", "math").unwrap();
        fs::write(root.join("unlisted.svg"), b"new required diagram").unwrap();
        assert!(matches!(
            write_custom_tree(&root, &manifest, &mut Vec::new()),
            Err(ExportError::InvalidTree)
        ));
        fs::remove_file(root.join("unlisted.svg")).unwrap();
        fs::write(root.join("article.cbor"), b"tampered").unwrap();
        assert!(matches!(
            write_custom_tree(&root, &manifest, &mut Vec::new()),
            Err(ExportError::Verify(ObjectVerifyError::HashMismatch(_)))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_staged_symlinks_and_hardlinks() {
        use std::os::unix::fs::symlink;
        let root = root();
        let outside = root.with_extension("outside");
        fs::write(&outside, b"article").unwrap();
        let target = root.join("article.cbor");
        fs::hard_link(&outside, &target).unwrap();
        assert!(matches!(
            custom_manifest_from_tree(&root, "math", "enwiki", "20261001", "math"),
            Err(ExportError::InvalidTree)
        ));
        fs::remove_file(&target).unwrap();
        symlink(&outside, &target).unwrap();
        assert!(matches!(
            custom_manifest_from_tree(&root, "math", "enwiki", "20261001", "math"),
            Err(ExportError::InvalidTree)
        ));
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }
}
