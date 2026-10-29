//! Bounded, streaming verification of all declared pack objects in a staging tree.
//!
//! This does not parse a .wpack archive or authenticate a publisher. Callers
//! must hold a private immutable staging tree during verification and install.

use crate::manifest::{Manifest, ManifestError, MAX_DECLARED_BYTES};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug)]
pub enum ObjectVerifyError {
    Manifest(ManifestError),
    UnsafePath,
    UnexpectedObject(String),
    Io(io::Error),
    SizeMismatch(String),
    HashMismatch(String),
    ResourceBudget,
}

/// Verify *every* object in a validated manifest before a caller can treat
/// it as locally complete. The input manifest is not authenticated here.
/// Returns the measured byte total; unlisted files and directories are rejected.
///
/// The caller must prevent concurrent writes/renames of the staging tree.
/// Path inspection alone cannot defeat a writer racing the verifier.
pub fn verify_manifest_directory(
    manifest: &Manifest,
    root: &Path,
) -> Result<u64, ObjectVerifyError> {
    manifest.validate().map_err(ObjectVerifyError::Manifest)?;
    // Even when the immediate root is a real directory, a symlink higher in
    // its path may redirect object reads outside the caller-owned staging tree.
    // This assumes no concurrent attacker can rename any ancestor.
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(ObjectVerifyError::Io)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(ObjectVerifyError::UnsafePath);
        }
    }
    let root_meta = fs::symlink_metadata(root).map_err(ObjectVerifyError::Io)?;
    if !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err(ObjectVerifyError::UnsafePath);
    }

    // A digest-complete manifest does not authorize additional files or
    // directories. Reject undeclared entries before hashing any payloads,
    // including empty directories, symlinks and other special files.
    let mut expected_files = BTreeSet::new();
    let mut expected_dirs = BTreeSet::new();
    for object in &manifest.objects {
        expected_files.insert(object.path.as_str());
        let mut path = object.path.as_str();
        while let Some((directory, _)) = path.rsplit_once('/') {
            expected_dirs.insert(directory.to_owned());
            path = directory;
        }
    }
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(ObjectVerifyError::Io)? {
            let entry = entry.map_err(ObjectVerifyError::Io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| ObjectVerifyError::UnsafePath)?;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type().map_err(ObjectVerifyError::Io)?;
            if kind.is_dir() && expected_dirs.contains(&relative) {
                pending.push((entry.path(), relative));
            } else if !(kind.is_file() && expected_files.contains(relative.as_str())) {
                return Err(ObjectVerifyError::UnexpectedObject(relative));
            }
        }
    }

    let mut total = 0u64;
    for object in &manifest.objects {
        let mut path = root.to_path_buf();
        let parts: Vec<_> = object.path.split('/').collect();
        let mut leaf_meta = None;
        for (index, component) in parts.iter().enumerate() {
            path.push(component);
            let meta = fs::symlink_metadata(&path).map_err(ObjectVerifyError::Io)?;
            let is_leaf = index + 1 == parts.len();
            if meta.file_type().is_symlink()
                || (is_leaf && !meta.file_type().is_file())
                || (!is_leaf && !meta.file_type().is_dir())
            {
                return Err(ObjectVerifyError::UnsafePath);
            }
            if is_leaf {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.nlink() != 1 {
                        return Err(ObjectVerifyError::UnsafePath);
                    }
                }
                leaf_meta = Some(meta);
            }
        }
        let before = leaf_meta.ok_or(ObjectVerifyError::UnsafePath)?;
        if before.len() != object.bytes {
            return Err(ObjectVerifyError::SizeMismatch(object.path.clone()));
        }
        let mut file = File::open(&path).map_err(ObjectVerifyError::Io)?;
        let opened = file.metadata().map_err(ObjectVerifyError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if opened.dev() != before.dev() || opened.ino() != before.ino() || opened.nlink() != 1 {
                return Err(ObjectVerifyError::UnsafePath);
            }
        }
        if !opened.is_file() || opened.len() != object.bytes {
            return Err(ObjectVerifyError::SizeMismatch(object.path.clone()));
        }

        let mut hasher = Sha256::new();
        let mut seen = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = match file.read(&mut buffer) {
                Ok(n) => n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(ObjectVerifyError::Io(error)),
            };
            if n == 0 {
                break;
            }
            seen = seen
                .checked_add(n as u64)
                .ok_or(ObjectVerifyError::ResourceBudget)?;
            if seen > object.bytes {
                return Err(ObjectVerifyError::SizeMismatch(object.path.clone()));
            }
            hasher.update(&buffer[..n]);
        }
        if seen != object.bytes
            || file.metadata().map_err(ObjectVerifyError::Io)?.len() != object.bytes
        {
            return Err(ObjectVerifyError::SizeMismatch(object.path.clone()));
        }
        if !format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(&object.sha256) {
            return Err(ObjectVerifyError::HashMismatch(object.path.clone()));
        }
        total = total
            .checked_add(seen)
            .ok_or(ObjectVerifyError::ResourceBudget)?;
        if total > MAX_DECLARED_BYTES {
            return Err(ObjectVerifyError::ResourceBudget);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Object, Origin, FORMAT_VERSION};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn staging_dir() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wpack-objects-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }

    fn declared(path: &str, content: &[u8]) -> Object {
        Object {
            path: path.into(),
            bytes: content.len() as u64,
            sha256: format!("{:x}", Sha256::digest(content)),
        }
    }

    fn manifest(objects: Vec<Object>) -> Manifest {
        Manifest {
            version: FORMAT_VERSION,
            pack_id: "math".into(),
            project: "enwiki".into(),
            snapshot: "20261001".into(),
            origin: Origin::Custom {
                definition_id: "math".into(),
            },
            objects,
        }
    }

    #[test]
    fn full_staged_object_set_verifies_without_network_or_loading_archive() {
        let root = staging_dir();
        let article = b"small canonical compressed article frame";
        let figure = b"<svg>offline figure</svg>";
        fs::create_dir(root.join("articles")).unwrap();
        fs::create_dir(root.join("media")).unwrap();
        fs::write(root.join("articles/42.pack"), article).unwrap();
        fs::write(root.join("media/chart.svg"), figure).unwrap();
        let expected = manifest(vec![
            declared("media/chart.svg", figure),
            declared("articles/42.pack", article),
        ]);
        assert_eq!(
            verify_manifest_directory(&expected, &root).unwrap(),
            (article.len() + figure.len()) as u64
        );
        fs::write(root.join("media/chart.svg"), b"<svg>offline figuRe</svg>").unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::HashMismatch(_))
        ));
        fs::remove_file(root.join("media/chart.svg")).unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::Io(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_incomplete_sizes_and_manifest_path_collisions() {
        let root = staging_dir();
        fs::write(root.join("record.pack"), b"bytes").unwrap();
        let mut expected = manifest(vec![declared("record.pack", b"bytes")]);
        expected.objects[0].bytes += 1;
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::SizeMismatch(_))
        ));
        expected.objects.push(declared("record.pack/evil", b"bad"));
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::Manifest(ManifestError::DuplicateObject))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_object_tree_with_symlinked_grandparent() {
        use std::os::unix::fs::symlink;

        let base = staging_dir();
        let real = base.join("real");
        let leaf = real.join("nested");
        fs::create_dir_all(&leaf).unwrap();
        let bytes = b"diagram bytes";
        fs::write(leaf.join("plot.svg"), bytes).unwrap();
        let alias = base.join("alias");
        symlink(&real, &alias).unwrap();
        let expected = manifest(vec![declared("plot.svg", bytes)]);
        // The leaf is a real directory, while a higher component is a
        // symlink. Both must be checked, not only the final root metadata.
        assert!(matches!(
            verify_manifest_directory(&expected, &alias.join("nested")),
            Err(ObjectVerifyError::UnsafePath)
        ));
        assert_eq!(
            verify_manifest_directory(&expected, &leaf).unwrap(),
            bytes.len() as u64
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_root_parent_and_hardlinked_external_object() {
        use std::os::unix::fs::symlink;

        let root = staging_dir();
        let bytes = b"licensed figure bytes";
        let source = root.with_extension("external");
        fs::write(&source, bytes).unwrap();
        fs::create_dir(root.join("media")).unwrap();
        let target = root.join("media/chart.svg");
        fs::hard_link(&source, &target).unwrap();
        let expected = manifest(vec![declared("media/chart.svg", bytes)]);
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::UnsafePath)
        ));
        fs::remove_file(&target).unwrap();
        symlink(&source, &target).unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::UnsafePath)
        ));
        fs::remove_file(&target).unwrap();
        fs::remove_dir(root.join("media")).unwrap();
        symlink(root.parent().unwrap(), root.join("media")).unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::UnsafePath)
        ));
        let alias = root.with_extension("link");
        symlink(&root, &alias).unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &alias),
            Err(ObjectVerifyError::UnsafePath)
        ));
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(source).unwrap();
    }
    #[test]
    fn rejects_unlisted_staging_files_and_empty_directories() {
        let root = staging_dir();
        let bytes = b"offline diagram";
        fs::create_dir(root.join("media")).unwrap();
        fs::write(root.join("media/chart.svg"), bytes).unwrap();
        let expected = manifest(vec![declared("media/chart.svg", bytes)]);
        assert_eq!(
            verify_manifest_directory(&expected, &root).unwrap(),
            bytes.len() as u64
        );

        fs::write(root.join("unexpected.txt"), b"not in manifest").unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::UnexpectedObject(path)) if path == "unexpected.txt"
        ));
        fs::remove_file(root.join("unexpected.txt")).unwrap();

        fs::create_dir(root.join("empty")).unwrap();
        assert!(matches!(
            verify_manifest_directory(&expected, &root),
            Err(ObjectVerifyError::UnexpectedObject(path)) if path == "empty"
        ));
        fs::remove_dir(root.join("empty")).unwrap();
        assert_eq!(
            verify_manifest_directory(&expected, &root).unwrap(),
            bytes.len() as u64
        );
        fs::remove_dir_all(root).unwrap();
    }
}
