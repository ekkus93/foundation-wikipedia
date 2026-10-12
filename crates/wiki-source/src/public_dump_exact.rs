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
                if !files.contains(relative.as_str()) {
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
