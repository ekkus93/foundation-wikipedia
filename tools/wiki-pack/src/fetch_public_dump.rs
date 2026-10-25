//! Fetch and atomically stage the exact official SHA256SUMS inventory only.
//! The independently supplied SHA-256 is an integrity pin, not a Wikimedia
//! publisher signature, completeness proof or permission to activate a pack.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use wiki_source::public_dump_https::InventoryCurl;
use wiki_source::public_dump_origin::is_official_checksum_url;
use wiki_source::public_dump_pinned::parse_pinned_inventory;

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

fn destination_available(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err("SHA256SUMS already exists; refusing to replace existing source".into()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("cannot inspect SHA256SUMS destination: {err}")),
    }
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
    if expected_sha256.len() != 64
        || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("invalid independently supplied SHA256SUMS digest".into());
    }
    safe_private_root(root)?;
    destination_available(&root.join("SHA256SUMS"))?;
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
    let count = run_with_fetch(project, generation, expected_sha256, root, |wiki, date, sha| {
        InventoryCurl::new()
            .fetch_verified_inventory(wiki, date, sha)
            .map_err(|error| format!("official HTTPS fetch rejected: {error:?}"))
    })?;
    println!("Inventory SHA256SUMS staged and digest-verified: {count} declared members");
    println!("Wikimedia TLS transport: certificate-verified HTTPS, no redirects");
    println!("Independent publisher provenance: NOT AUTHENTICATED");
    println!("Expected census, compressed members and source completeness: NOT VERIFIED");
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
        format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n",
            "a".repeat(64),
        )
        .into_bytes()
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
        }).unwrap();
        assert_eq!(count, 1);
        assert_eq!(fetches, 1);
        assert_eq!(fs::read(root.join("SHA256SUMS")).unwrap(), raw);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.join("SHA256SUMS")).unwrap().permissions().mode() & 0o777,
                0o600,
            );
        }
        assert!(run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
            fetches += 1;
            panic!("occupied destination must avoid network access")
        }).is_err());
        assert_eq!(fetches, 1);
        assert_eq!(fs::read(root.join("SHA256SUMS")).unwrap(), raw);
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
            }).is_err());
        }
        assert!(run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
            Ok(b"forged".to_vec())
        }).is_err());
        assert!(run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
            Err("transport failure".into())
        }).is_err());
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
        assert!(run_with_fetch("enwiki", "2026-10-01", &digest, &alias, |_, _, _| {
            panic!("symlinked root must fail before download")
        }).is_err());
        let target = root.join("missing-inventory");
        symlink(target, root.join("SHA256SUMS")).unwrap();
        assert!(run_with_fetch("enwiki", "2026-10-01", &digest, &root, |_, _, _| {
            panic!("dangling symlink output must fail before download")
        }).is_err());
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
