//! Require an exact declared-member tree for digest-pinned public dumps.
//! Callers must separately authenticate the inventory and keep staging private.

use crate::public_dump_pinned::{verify_pinned_staged_export, PinnedInventory};
use crate::public_dump_stage::StagedDumpError;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Reject unlisted entries after validating all declared file digests.
/// Filesystem preflight cannot prevent concurrent untrusted changes.
pub fn verify_pinned_staged_export_exact(
    root: &Path,
    inventory: &PinnedInventory,
    max_member_bytes: u64,
) -> Result<u64, StagedDumpError> {
    let bytes = verify_pinned_staged_export(root, inventory, max_member_bytes)?;
    let files: BTreeSet<&str> = inventory
        .members()
        .iter()
        .map(|member| member.relative_path.as_str())
        .collect();
    let mut directories = BTreeSet::new();
    for member in inventory.members() {
        let mut prefix = String::new();
        let mut parts = member.relative_path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            directories.insert(prefix.clone());
        }
    }
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(StagedDumpError::Io)? {
            let entry = entry.map_err(StagedDumpError::Io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| StagedDumpError::InvalidInventory)?;
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type().map_err(StagedDumpError::Io)?;
            if kind.is_symlink() {
                return Err(StagedDumpError::UnsafeStagingPath);
            }
            if kind.is_file() {
                // The underlying verifier permits these only as a pair and
                // rejects symlinks/hardlinks. Do not reject its own valid
                // publication sidecars during this second exact-tree walk.
                let publication_sidecar =
                    matches!(relative.as_str(), "SHA256SUMS" | "PUBLICATION.json");
                if !publication_sidecar && !files.contains(relative.as_str()) {
                    return Err(StagedDumpError::InvalidInventory);
                }
            } else if kind.is_dir() {
                if !directories.contains(&relative) {
                    return Err(StagedDumpError::InvalidInventory);
                }
                pending.push((entry.path(), relative));
            } else {
                return Err(StagedDumpError::UnsafeStagingPath);
            }
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_pinned::parse_pinned_inventory;
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn accepts_paired_publication_sidecars_but_rejects_extra_files() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("wiki-dump-exact-{}-{stamp}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let bytes = b"verified fixture member";
        let inventory_bytes = format!("{:x}  {name}\n", Sha256::digest(bytes));
        let digest = format!("{:x}", Sha256::digest(inventory_bytes.as_bytes()));
        let inventory =
            parse_pinned_inventory(inventory_bytes.as_bytes(), "enwiki", "2026-10-01", &digest)
                .unwrap();
        fs::write(root.join(name), bytes).unwrap();
        assert_eq!(
            verify_pinned_staged_export_exact(&root, &inventory, 4096).unwrap(),
            bytes.len() as u64
        );
        fs::write(root.join("SHA256SUMS"), inventory_bytes).unwrap();
        assert!(verify_pinned_staged_export_exact(&root, &inventory, 4096).is_err());
        fs::write(root.join("PUBLICATION.json"), b"{}").unwrap();
        assert_eq!(
            verify_pinned_staged_export_exact(&root, &inventory, 4096).unwrap(),
            bytes.len() as u64
        );
        fs::write(root.join("rogue.txt"), b"unexpected").unwrap();
        assert!(verify_pinned_staged_export_exact(&root, &inventory, 4096).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
