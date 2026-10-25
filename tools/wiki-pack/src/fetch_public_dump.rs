//! Fetch and atomically stage the exact official SHA256SUMS inventory only.
//! The independently supplied SHA-256 is an integrity pin, not a Wikimedia
//! publisher signature, completeness proof or permission to activate a pack.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use wiki_source::public_dump_https::{
    verified_existing_member, InventoryCurl, MAX_INVENTORY_BYTES,
};
use wiki_source::public_dump_origin::is_official_checksum_url;
use wiki_source::public_dump_pinned::{parse_pinned_inventory, PinnedInventory};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn safe_private_root(root: &Path) -> Result<(), String> {
    for parent in root.ancestors() {
        if parent.as_os_str().is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(parent)
            .map_err(|e| format!("cannot inspect staging directory: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err("private staging directory cannot contain symlink components".into());
        }
    }
    let metadata = fs::symlink_metadata(root)
        .map_err(|e| format!("cannot inspect private staging root: {e}"))?;
    if !metadata.file_type().is_dir() {
        return Err("staging root must be a directory".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("staging root must be private (mode 0700)".into());
        }
    }
    Ok(())
}

fn publish_inventory(root: &Path, raw: &[u8]) -> Result<(), String> {
    // Caller validated the complete digest-pinned inventory before writing.
    // The staged file remains private and unpublished until it is synced.
    let destination = root.join("SHA256SUMS");
    let (temporary, mut output) = loop {
        let candidate = root.join(format!(
            ".wiki-public-inventory-{}-{}.tmp",
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
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot stage SHA256SUMS: {error}")),
        }
    };
    let mut published = false;
    let operation = (|| -> io::Result<()> {
        output.write_all(raw)?;
        output.sync_all()?;
        drop(output);
        // A hard link is atomic and refuses a concurrently created target.
        fs::hard_link(&temporary, &destination)?;
        published = true;
        fs::remove_file(&temporary)?;
        #[cfg(unix)]
        File::open(root)?.sync_all()?;
        Ok(())
    })();
    if let Err(error) = operation {
        if published {
            let _ = fs::remove_file(&destination);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot publish SHA256SUMS: {error}"));
    }
    Ok(())
}

fn run_with_fetch<F>(
    project: &str,
    generation: &str,
    expected_sha256: &str,
    root: &Path,
    fetch: F,
) -> Result<usize, String>
where
    F: FnOnce(&str, &str, &str) -> Result<Vec<u8>, String>,
{
    let checksum_url = format!(
        "https://dumps.wikimedia.org/other/mediawiki_content_current/{project}/{generation}/xml/bzip2/SHA256SUMS"
    );
    if !is_official_checksum_url(&checksum_url, project, generation) {
        return Err("invalid Wikimedia dump identity".into());
    }
    if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid independently supplied SHA256SUMS digest".into());
    }
    safe_private_root(root)?;
    // A retry reuses only an exact digest-pinned, privately staged inventory.
    // Occupied malformed or linked files are never overwritten or refetched.
    match fs::symlink_metadata(root.join("SHA256SUMS")) {
        Ok(_) => {
            return read_staged_inventory(root, project, generation, expected_sha256)
                .map(|inventory| inventory.members().len());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect SHA256SUMS destination: {error}")),
    }
    let raw = fetch(project, generation, expected_sha256)?;
    let inventory = parse_pinned_inventory(&raw, project, generation, expected_sha256)
        .map_err(|error| format!("downloaded SHA256SUMS rejected: {error:?}"))?;
    publish_inventory(root, &raw)?;
    Ok(inventory.members().len())
}

pub fn run(
    project: &str,
    generation: &str,
    expected_sha256: &str,
    root: &Path,
) -> Result<(), String> {
    let count = run_with_fetch(
        project,
        generation,
        expected_sha256,
        root,
        |wiki, date, sha| {
            InventoryCurl::new()
                .fetch_verified_inventory(wiki, date, sha)
                .map_err(|error| format!("official HTTPS fetch rejected: {error:?}"))
        },
    )?;
    println!("Inventory SHA256SUMS staged and digest-verified: {count} declared members");
    println!("Wikimedia TLS transport: certificate-verified HTTPS, no redirects");
    println!("Independent publisher provenance: NOT AUTHENTICATED");
    println!("Expected census, compressed members and source completeness: NOT VERIFIED");
    println!("No pack installed or active snapshot changed.");
    Ok(())
}

/// Read a locally pinned official checksum inventory before requesting any
/// compressed member. This deliberately does not accept an arbitrary URL,
/// download digest or caller-created member descriptor.
fn read_staged_inventory(
    root: &Path,
    project: &str,
    generation: &str,
    digest: &str,
) -> Result<PinnedInventory, String> {
    safe_private_root(root)?;
    let path = root.join("SHA256SUMS");
    let inspected = fs::symlink_metadata(&path)
        .map_err(|e| format!("cannot inspect staged SHA256SUMS: {e}"))?;
    if !inspected.file_type().is_file()
        || inspected.len() == 0
        || inspected.len() > MAX_INVENTORY_BYTES as u64
    {
        return Err("invalid source inventory size or type".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if inspected.nlink() != 1 {
            return Err("source inventory hard-link aliases are unsafe".into());
        }
    }
    let file = File::open(&path).map_err(|e| format!("cannot open staged SHA256SUMS: {e}"))?;
    let opened = file
        .metadata()
        .map_err(|e| format!("inventory metadata: {e}"))?;
    if !opened.is_file() || opened.len() != inspected.len() {
        return Err("source inventory changed during open".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != inspected.dev() || opened.ino() != inspected.ino() || opened.nlink() != 1
        {
            return Err("source inventory identity changed during open".into());
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_INVENTORY_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read staged SHA256SUMS: {e}"))?;
    if bytes.len() as u64 != inspected.len() {
        return Err("source inventory changed during read".into());
    }
    parse_pinned_inventory(&bytes, project, generation, digest)
        .map_err(|e| format!("pinned staged SHA256SUMS rejected: {e:?}"))
}

/// Audit the entire local acquisition tree after a resumable download.
/// The digest pin is supplied independently; this does not authenticate
/// Wikimedia publication, render articles or authorize pack installation.
fn verify_local_generation(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    max_member_bytes: u64,
) -> Result<(usize, u64), String> {
    if !(1..=1_u64 << 40).contains(&max_member_bytes) {
        return Err("compressed member byte limit must be 1..=1TiB".into());
    }
    let inventory = read_staged_inventory(root, project, generation, inventory_sha256)?;
    let mut total = 0u64;
    let mut files = BTreeSet::from([PathBuf::from("SHA256SUMS")]);
    let mut directories = BTreeSet::new();
    for (index, member) in inventory.members().iter().enumerate() {
        let bytes = verified_existing_member(&inventory, index, root, max_member_bytes)
            .map_err(|error| format!("member {index} rejected: {error:?}"))?
            .ok_or_else(|| format!("member {index} is missing"))?;
        total = total
            .checked_add(bytes)
            .filter(|sum| *sum <= wiki_source::MAX_SOURCE_GENERATION_BYTES)
            .ok_or_else(|| "compressed generation byte budget exceeded".to_owned())?;
        let relative = PathBuf::from(&member.relative_path);
        for ancestor in relative.ancestors().skip(1) {
            if !ancestor.as_os_str().is_empty() {
                directories.insert(ancestor.to_path_buf());
            }
        }
        files.insert(relative);
    }

    // Check the complete directory tree, not just the named members. This
    // rejects unfinished .part checkpoints, stray payloads and symlink aliases
    // instead of mistaking a dirty transport staging tree for a clean export.
    let mut pending = vec![(root.to_path_buf(), PathBuf::new())];
    while let Some((directory, prefix)) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("cannot inspect staged member tree: {error}"))?
        {
            let entry = entry.map_err(|error| format!("invalid staging entry: {error}"))?;
            let relative = prefix.join(entry.file_name());
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| format!("cannot inspect staging entry: {error}"))?;
            if metadata.file_type().is_file() {
                if !files.remove(&relative) {
                    return Err("unlisted or duplicated file in staged generation".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.nlink() != 1 {
                        return Err("hardlinked file in staged generation".into());
                    }
                }
            } else if metadata.file_type().is_dir() {
                if !directories.remove(&relative) {
                    return Err("unlisted directory in staged generation".into());
                }
                pending.push((entry.path(), relative));
            } else {
                return Err("nonregular entry in staged generation".into());
            }
        }
    }
    if !files.is_empty() || !directories.is_empty() {
        return Err("staged generation is missing declared entries".into());
    }
    Ok((inventory.members().len(), total))
}

/// Read-only post-download integrity gate. The exact pinned SHA256SUMS and
/// every declared compressed member must be present, with no extra entries.
pub fn verify_public_members(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    max_member_bytes: u64,
) -> Result<(), String> {
    let (count, bytes) = verify_local_generation(
        project,
        generation,
        inventory_sha256,
        root,
        max_member_bytes,
    )?;
    println!("Verified {count} locally staged compressed members ({bytes} bytes)");
    println!("All declared SHA-256 digests and clean staging tree: VERIFIED");
    println!("Publisher provenance, independent census and rendering: NOT VERIFIED");
    println!("No pack installed or active snapshot changed.");
    Ok(())
}

fn run_member_with_fetch<F>(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    member_index: usize,
    max_bytes: u64,
    fetch: F,
) -> Result<u64, String>
where
    F: FnOnce(&PinnedInventory, usize, &Path, u64) -> Result<u64, String>,
{
    if !(1..=1_u64 << 40).contains(&max_bytes) {
        return Err("compressed member byte limit must be 1..=1TiB".into());
    }
    let inventory = read_staged_inventory(root, project, generation, inventory_sha256)?;
    if inventory.members().get(member_index).is_none() {
        return Err("member index is not declared by the pinned inventory".into());
    }
    fetch(&inventory, member_index, root, max_bytes)
}

pub fn run_member(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    member_index: usize,
    max_bytes: u64,
) -> Result<(), String> {
    let downloaded = run_member_with_fetch(
        project,
        generation,
        inventory_sha256,
        root,
        member_index,
        max_bytes,
        |inventory, index, directory, byte_limit| {
            InventoryCurl::new()
                .stage_verified_member(inventory, index, directory, byte_limit)
                .map_err(|error| format!("official compressed member rejected: {error:?}"))
        },
    )?;
    println!("SHA-256-verified compressed member staged: {downloaded} bytes");
    println!("Expected census and Wikimedia publisher provenance: NOT AUTHENTICATED");
    println!("No pack installed or active snapshot changed.");
    Ok(())
}

/// Download every member of one digest-pinned generation in page-range order.
/// A failure stops before the next shard; already verified shards remain
/// reusable on the next invocation. This is transport, not source acceptance.
fn run_all_members_with_fetch<F>(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    max_bytes: u64,
    mut fetch: F,
) -> Result<(usize, u64), String>
where
    F: FnMut(&PinnedInventory, usize, &Path, u64) -> Result<u64, String>,
{
    if !(1..=1_u64 << 40).contains(&max_bytes) {
        return Err("compressed member byte limit must be 1..=1TiB".into());
    }
    let inventory = read_staged_inventory(root, project, generation, inventory_sha256)?;
    let mut total = 0u64;
    for index in 0..inventory.members().len() {
        let bytes = fetch(&inventory, index, root, max_bytes)
            .map_err(|error| format!("member {index} failed: {error}"))?;
        total = total
            .checked_add(bytes)
            .filter(|sum| *sum <= wiki_source::MAX_SOURCE_GENERATION_BYTES)
            .ok_or_else(|| "compressed generation byte budget exceeded".to_owned())?;
    }
    Ok((inventory.members().len(), total))
}

pub fn run_all_members(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    max_bytes: u64,
) -> Result<(), String> {
    let client = InventoryCurl::new();
    let (count, total) = run_all_members_with_fetch(
        project,
        generation,
        inventory_sha256,
        root,
        max_bytes,
        |inventory, index, directory, limit| {
            client
                .stage_verified_member(inventory, index, directory, limit)
                .map_err(|error| format!("official compressed member rejected: {error:?}"))
        },
    )?;
    println!("Staged {count} SHA-256-verified compressed members ({total} bytes)");
    println!("Wikimedia publisher provenance and independent census: NOT AUTHENTICATED");
    println!("No pack installed or active snapshot changed.");
    Ok(())
}

/** Resume one exact pinned member through bounded HTTP 206 byte ranges.
 * Partial chunks remain private and are never accepted as complete until
 * the entire compressed member passes the pinned SHA-256 publication gate.
 */
pub fn run_resumed_member(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    member_index: usize,
    max_bytes: u64,
) -> Result<(), String> {
    let bytes = run_member_with_fetch(
        project,
        generation,
        inventory_sha256,
        root,
        member_index,
        max_bytes,
        |inventory, index, directory, limit| {
            InventoryCurl::new()
                .stage_resumed_member(inventory, index, directory, limit)
                .map_err(|error| format!("resumable official member rejected: {error:?}"))
        },
    )?;
    println!("SHA-256-verified compressed member staged/resumed: {bytes} bytes");
    println!("Publisher provenance, independent census and renderer fidelity: NOT VERIFIED");
    println!("No pack installed or active snapshot changed.");
    Ok(())
}

/// Resume all members in pinned page-range order, preserving complete and
/// incomplete verified-transfer checkpoints when a later range fails.
pub fn run_all_resumed_members(
    project: &str,
    generation: &str,
    inventory_sha256: &str,
    root: &Path,
    max_bytes: u64,
) -> Result<(), String> {
    let client = InventoryCurl::new();
    let (count, total) = run_all_members_with_fetch(
        project,
        generation,
        inventory_sha256,
        root,
        max_bytes,
        |inventory, index, directory, limit| {
            client
                .stage_resumed_member(inventory, index, directory, limit)
                .map_err(|error| format!("resumable official member rejected: {error:?}"))
        },
    )?;
    println!("Staged/resumed {count} SHA-256-verified compressed members ({total} bytes)");
    println!("Publisher provenance, independent census and renderer fidelity: NOT VERIFIED");
    println!("No pack installed or active snapshot changed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn private_root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wiki-cli-inventory-{}-{}",
            std::process::id(),
            TEST_DIR.fetch_add(1, AtomicOrdering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        }
        root
    }

    fn valid_raw() -> Vec<u8> {
        format!("{}  enwiki-2026-10-01-p1p100.xml.bz2\n", "a".repeat(64),).into_bytes()
    }

    #[test]
    fn stages_verbatim_verified_inventory_with_no_overwrite_and_bounded_output() {
        let root = private_root();
        let raw = valid_raw();
        let digest = format!("{:x}", Sha256::digest(&raw));
        let mut fetches = 0usize;
        let count = run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
            fetches += 1;
            Ok(raw.clone())
        })
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(fetches, 1);
        assert_eq!(fs::read(root.join("SHA256SUMS")).unwrap(), raw);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.join("SHA256SUMS"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600,
            );
        }
        assert_eq!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
                fetches += 1;
                panic!("verified inventory retry must avoid network access")
            })
            .unwrap(),
            1
        );
        assert_eq!(fetches, 1);
        assert_eq!(fs::read(root.join("SHA256SUMS")).unwrap(), raw);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn occupied_unverified_inventory_fails_closed_without_transport_or_overwrite() {
        let root = private_root();
        let raw = valid_raw();
        let digest = format!("{:x}", Sha256::digest(&raw));
        let path = root.join("SHA256SUMS");
        fs::write(&path, &raw).unwrap();

        for (project, date, pin) in [
            ("frwiki", "2026-10-01", digest.as_str()),
            ("enwiki", "2026-10-02", digest.as_str()),
            ("enwiki", "2026-10-01", "0"),
        ] {
            assert!(run_with_fetch(project, date, pin, &root, |_, _, _| {
                panic!("mismatched source identity must not initiate transport")
            })
            .is_err());
        }

        fs::write(&path, b"tampered").unwrap();
        assert!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
                panic!("corrupt occupied inventory must not initiate transport")
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"tampered");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn occupied_hardlinked_inventory_is_not_accepted_as_cached() {
        let root = private_root();
        let raw = valid_raw();
        let digest = format!("{:x}", Sha256::digest(&raw));
        let path = root.join("SHA256SUMS");
        let outside = root.with_extension("outside-inventory");
        fs::write(&outside, &raw).unwrap();
        fs::hard_link(&outside, &path).unwrap();
        assert!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
                panic!("hardlinked existing inventory must not initiate transport")
            })
            .is_err()
        );
        assert_eq!(fs::read(&outside).unwrap(), raw);
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn local_generation_gate_requires_every_digest_and_a_clean_tree() {
        let root = private_root();
        let first = "enwiki-2026-10-01-p1p100.xml.bz2";
        let second = "enwiki-2026-10-01-p101p200.xml.bz2";
        let first_body = b"fixture-one";
        let second_body = b"fixture-two";
        let raw = format!(
            "{:x}  {first}\n{:x}  {second}\n",
            Sha256::digest(first_body),
            Sha256::digest(second_body),
        );
        let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
        fs::write(root.join("SHA256SUMS"), &raw).unwrap();
        fs::write(root.join(first), first_body).unwrap();
        assert!(verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).is_err());
        fs::write(root.join(second), second_body).unwrap();
        assert_eq!(
            verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).unwrap(),
            (2, (first_body.len() + second_body.len()) as u64)
        );
        assert!(verify_local_generation("frwiki", "2026-10-01", &pin, &root, 1024).is_err());
        assert!(verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1).is_err());
        fs::write(root.join(".fw-resume-orphan.part"), b"partial").unwrap();
        assert!(verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).is_err());
        fs::remove_file(root.join(".fw-resume-orphan.part")).unwrap();
        fs::write(root.join(second), b"tampered").unwrap();
        assert!(verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).is_err());
        fs::write(root.join(second), second_body).unwrap();
        assert_eq!(
            verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).unwrap().0,
            2
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn local_generation_gate_rejects_member_aliases_and_unlisted_symlinks() {
        use std::os::unix::fs::symlink;
        let root = private_root();
        let name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let body = b"fixture";
        let raw = format!("{:x}  {name}\n", Sha256::digest(body));
        let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
        fs::write(root.join("SHA256SUMS"), &raw).unwrap();
        let outside = root.with_extension("outside-member");
        fs::write(&outside, body).unwrap();
        fs::hard_link(&outside, root.join(name)).unwrap();
        assert!(verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).is_err());
        fs::remove_file(root.join(name)).unwrap();
        fs::write(root.join(name), body).unwrap();
        symlink(&outside, root.join("extra-link")).unwrap();
        assert!(verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).is_err());
        fs::remove_file(root.join("extra-link")).unwrap();
        assert_eq!(
            verify_local_generation("enwiki", "2026-10-01", &pin, &root, 1024).unwrap(),
            (1, body.len() as u64)
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_file(outside).unwrap();
    }

    #[test]
    fn member_selection_requires_verified_private_inventory_and_index() {
        let root = private_root();
        let raw = valid_raw();
        let digest = format!("{:x}", Sha256::digest(&raw));
        let mut requests = 0usize;
        let try_fetch = |inventory: &PinnedInventory, index: usize, _: &Path, limit: u64| {
            requests += 1;
            assert_eq!(
                inventory.members()[index].relative_path,
                "enwiki-2026-10-01-p1p100.xml.bz2"
            );
            assert_eq!(limit, 1024);
            Ok(4)
        };
        assert!(run_member_with_fetch(
            "enwiki",
            "2026-10-01",
            &digest,
            &root,
            0,
            1024,
            |_, _, _, _| panic!("missing inventory must fail before HTTP")
        )
        .is_err());
        fs::write(root.join("SHA256SUMS"), &raw).unwrap();
        assert_eq!(
            run_member_with_fetch("enwiki", "2026-10-01", &digest, &root, 0, 1024, try_fetch,)
                .unwrap(),
            4
        );
        assert_eq!(requests, 1);
        for (project, date, pin, index, limit) in [
            ("frwiki", "2026-10-01", digest.as_str(), 0, 1024),
            ("enwiki", "2026-10-02", digest.as_str(), 0, 1024),
            ("enwiki", "2026-10-01", "wrong", 0, 1024),
            ("enwiki", "2026-10-01", digest.as_str(), 1, 1024),
            ("enwiki", "2026-10-01", digest.as_str(), 0, 0),
        ] {
            assert!(run_member_with_fetch(
                project,
                date,
                pin,
                &root,
                index,
                limit,
                |_, _, _, _| panic!("invalid member scope must fail before HTTP")
            )
            .is_err());
        }
        fs::write(root.join("SHA256SUMS"), b"tampered inventory").unwrap();
        assert!(run_member_with_fetch(
            "enwiki",
            "2026-10-01",
            &digest,
            &root,
            0,
            1024,
            |_, _, _, _| panic!("tampered inventory must fail before HTTP")
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_members_use_one_pinned_inventory_in_page_order_and_stop_on_error() {
        let root = private_root();
        let first = "enwiki-2026-10-01-p1p100.xml.bz2";
        let second = "enwiki-2026-10-01-p101p200.xml.bz2";
        // Deliberately reverse the checksum line order: pinned inventory
        // normalizes to ascending page ranges before download.
        let raw = format!(
            "{}  {second}\n{}  {first}\n",
            "b".repeat(64),
            "a".repeat(64)
        );
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        fs::write(root.join("SHA256SUMS"), &raw).unwrap();
        let mut visited = Vec::new();
        let result = run_all_members_with_fetch(
            "enwiki",
            "2026-10-01",
            &digest,
            &root,
            1024,
            |inventory, index, _, limit| {
                assert_eq!(limit, 1024);
                visited.push(inventory.members()[index].relative_path.clone());
                Ok(index as u64 + 1)
            },
        )
        .unwrap();
        assert_eq!(result, (2, 3));
        assert_eq!(visited, [first, second]);

        let mut attempts = 0;
        assert!(run_all_members_with_fetch(
            "enwiki",
            "2026-10-01",
            &digest,
            &root,
            1024,
            |_, _, _, _| {
                attempts += 1;
                Err("synthetic transport failure".into())
            },
        )
        .is_err());
        assert_eq!(attempts, 1);
        for (project, pin, limit) in [
            ("frwiki", digest.as_str(), 1024),
            ("enwiki", "wrong", 1024),
            ("enwiki", digest.as_str(), 0),
        ] {
            assert!(run_all_members_with_fetch(
                project,
                "2026-10-01",
                pin,
                &root,
                limit,
                |_, _, _, _| panic!("invalid preflight must not fetch"),
            )
            .is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resumed_generation_retry_preserves_verified_shards_and_continues_partial_second() {
        use wiki_source::public_dump_https::{RangeFetchError, VerifiedRange};
        use wiki_source::public_dump_resume::stage_resumed_member_with;

        let root = private_root();
        let first = "enwiki-2026-10-01-p1p100.xml.bz2";
        let second = "enwiki-2026-10-01-p101p200.xml.bz2";
        let raw = format!(
            "{:x}  {second}\n{:x}  {first}\n",
            Sha256::digest(b"ghijkl"),
            Sha256::digest(b"abcdef")
        );
        let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
        fs::write(root.join("SHA256SUMS"), raw).unwrap();

        // The first shard finishes; the second leaves only a private synced
        // prefix when transport fails. The generation is not declared done.
        let mut first_attempt = Vec::new();
        let failed = run_all_members_with_fetch(
            "enwiki",
            "2026-10-01",
            &digest,
            &root,
            64,
            |inventory, index, directory, limit| {
                stage_resumed_member_with(inventory, index, directory, limit, 3, |start, _, _| {
                    first_attempt.push((index, start));
                    match (index, start) {
                        (0, 0) => Ok(VerifiedRange {
                            total_bytes: 6,
                            bytes: b"abc".to_vec(),
                        }),
                        (0, 3) => Ok(VerifiedRange {
                            total_bytes: 6,
                            bytes: b"def".to_vec(),
                        }),
                        (1, 0) => Ok(VerifiedRange {
                            total_bytes: 6,
                            bytes: b"ghi".to_vec(),
                        }),
                        (1, 3) => Err(RangeFetchError::IoFailure),
                        _ => panic!("unexpected first-attempt member/offset"),
                    }
                })
                .map_err(|error| format!("{error:?}"))
            },
        );
        assert!(failed.unwrap_err().contains("member 1 failed"));
        assert_eq!(first_attempt, [(0, 0), (0, 3), (1, 0), (1, 3)]);
        assert_eq!(fs::read(root.join(first)).unwrap(), b"abcdef");
        assert!(!root.join(second).exists());

        // Retry rehashes the completed first shard without transport, then
        // requests only the missing suffix of the second checkpoint.
        let mut retried = Vec::new();
        assert_eq!(
            run_all_members_with_fetch(
                "enwiki",
                "2026-10-01",
                &digest,
                &root,
                64,
                |inventory, index, directory, limit| {
                    stage_resumed_member_with(
                        inventory,
                        index,
                        directory,
                        limit,
                        3,
                        |start, _, _| {
                            retried.push((index, start));
                            assert_eq!((index, start), (1, 3));
                            Ok(VerifiedRange {
                                total_bytes: 6,
                                bytes: b"jkl".to_vec(),
                            })
                        },
                    )
                    .map_err(|error| format!("{error:?}"))
                },
            )
            .unwrap(),
            (2, 12)
        );
        assert_eq!(retried, [(1, 3)]);
        assert_eq!(fs::read(root.join(first)).unwrap(), b"abcdef");
        assert_eq!(fs::read(root.join(second)).unwrap(), b"ghijkl");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3);

        // A third invocation requires no HTTP for either member.
        assert_eq!(
            run_all_members_with_fetch(
                "enwiki",
                "2026-10-01",
                &digest,
                &root,
                64,
                |inventory, index, directory, limit| {
                    stage_resumed_member_with(inventory, index, directory, limit, 3, |_, _, _| {
                        panic!("both members must be reusable without transport")
                    })
                    .map_err(|error| format!("{error:?}"))
                },
            )
            .unwrap(),
            (2, 12)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_digests_sources_and_untrusted_bytes_cannot_publish() {
        let root = private_root();
        let raw = valid_raw();
        let digest = format!("{:x}", Sha256::digest(&raw));
        for (project, date, pin) in [
            ("enwiki", "2026-02-30", digest.as_str()),
            ("en/wiki", "2026-10-01", digest.as_str()),
            ("enwiki", "2026-10-01", "bad"),
        ] {
            assert!(run_with_fetch(project, date, pin, &root, |_, _, _| {
                panic!("invalid input must fail before network access")
            })
            .is_err());
        }
        assert!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
                Ok(b"forged".to_vec())
            })
            .is_err()
        );
        assert!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
                Err("transport failure".into())
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_roots_and_occupied_symlink_destinations_never_download() {
        use std::os::unix::fs::symlink;
        let root = private_root();
        let raw = valid_raw();
        let digest = format!("{:x}", Sha256::digest(&raw));
        let alias = root.with_extension("alias");
        symlink(&root, &alias).unwrap();
        assert!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &alias, |_, _, _| {
                panic!("symlinked root must fail before download")
            })
            .is_err()
        );
        let target = root.join("missing-inventory");
        symlink(target, root.join("SHA256SUMS")).unwrap();
        assert!(
            run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
                panic!("dangling symlink output must fail before download")
            })
            .is_err()
        );
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
