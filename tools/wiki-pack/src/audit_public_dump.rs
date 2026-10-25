//! Diagnostic-only native bzip2/XML shard census against a separately pinned
//! expected page-ID set. A caller-supplied SHA is integrity, NOT publisher trust.
//! This command never installs a pack, activates a snapshot or claims completeness.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use wiki_source::public_dump_census_gate::{
    stage_and_audit_native_bzip2_census, NativeCensusAudit,
};
use wiki_source::public_dump_pinned::parse_pinned_inventory;

const MAX_INVENTORY_BYTES: u64 = 32 * 1024 * 1024;
const MAX_MEMBER_BUDGET: u64 = 1_u64 << 40;

pub struct NativeDumpAudit<'a> {
    pub source_root: &'a Path,
    pub project: &'a str,
    pub generation: &'a str,
    pub inventory_sha256: &'a str,
    pub expected_path: &'a Path,
    pub expected_sha256: &'a str,
    pub namespace: i32,
    pub output_root: &'a Path,
    pub output_name: &'a str,
    pub max_compressed_bytes: u64,
    pub max_decoded_bytes: u64,
}

fn safe_directory(path: &Path) -> Result<std::path::PathBuf, String> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| format!("cannot inspect private directory: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("private directory path contains a symlink".into());
        }
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect private directory: {error}"))?;
    if !metadata.is_dir() {
        return Err("source and output roots must be directories".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("source and output roots must be private (mode 0700)".into());
        }
    }
    fs::canonicalize(path).map_err(|error| format!("cannot resolve private directory: {error}"))
}

fn open_regular_bounded(path: &Path, max_bytes: u64) -> io::Result<File> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if fs::symlink_metadata(ancestor)?.file_type().is_symlink() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "symlink in source path"));
        }
    }
    let inspected = fs::symlink_metadata(path)?;
    if !inspected.is_file() || inspected.len() == 0 || inspected.len() > max_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unsafe source member size or type"));
    }
    let file = File::open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() != inspected.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "source member changed at open"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if inspected.dev() != opened.dev()
            || inspected.ino() != opened.ino()
            || inspected.nlink() != 1
            || opened.nlink() != 1
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "source member inode/link changed"));
        }
    }
    Ok(file)
}

pub fn run(config: &NativeDumpAudit<'_>) -> Result<usize, String> {
    if config.namespace < 0
        || !(1..=MAX_MEMBER_BUDGET).contains(&config.max_compressed_bytes)
        || !(1..=MAX_MEMBER_BUDGET).contains(&config.max_decoded_bytes)
    {
        return Err("namespace must be nonnegative and both byte budgets must be 1..=1TiB".into());
    }
    let source = safe_directory(config.source_root)?;
    let output = safe_directory(config.output_root)?;
    if source.starts_with(&output) || output.starts_with(&source) {
        return Err("private source and output directories must not overlap".into());
    }
    let expected = fs::canonicalize(config.expected_path)
        .map_err(|error| format!("cannot resolve independent expected census: {error}"))?;
    if expected.starts_with(&source) || expected.starts_with(&output) {
        return Err("expected census must be outside both source and output directories".into());
    }
    let mut inventory_file = open_regular_bounded(
        &source.join("SHA256SUMS"),
        MAX_INVENTORY_BYTES,
    ).map_err(|error| format!("cannot open pinned SHA256SUMS: {error}"))?;
    let mut raw = Vec::new();
    inventory_file
        .by_ref()
        .take(MAX_INVENTORY_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|error| format!("cannot read pinned SHA256SUMS: {error}"))?;
    if raw.len() as u64 > MAX_INVENTORY_BYTES {
        return Err("SHA256SUMS grew beyond the inventory budget".into());
    }
    let pinned = parse_pinned_inventory(
        &raw,
        config.project,
        config.generation,
        config.inventory_sha256,
    ).map_err(|error| format!("pinned inventory rejected: {error:?}"))?;
    let (path, delta) = stage_and_audit_native_bzip2_census(
        &output,
        config.output_name,
        &pinned,
        config.namespace,
        NativeCensusAudit {
            expected_path: &expected,
            expected_sha256: config.expected_sha256,
            max_compressed_bytes: config.max_compressed_bytes,
            max_decoded_bytes: config.max_decoded_bytes,
        },
        |name| open_regular_bounded(&source.join(name), config.max_compressed_bytes),
    ).map_err(|error| format!("native bzip2/XML census audit rejected: {error:?}"))?;
    println!("Native bzip2/XML member checks and observed page-ID census: PASSED");
    println!("Exact observed/expected page IDs: {}", delta.observed_count);
    println!("Staged audited census: {}", path.display());
    println!("Wikimedia publisher and independent expected-census provenance: NOT AUTHENTICATED");
    println!("Full source generation, rendered HTML and mandatory offline media: NOT VERIFIED");
    println!("No pack was built or installed; no active snapshot was changed.");
    Ok(delta.observed_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn private(path: &Path) {
        fs::create_dir(path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

    #[test]
    fn cli_native_census_roundtrip_rejects_mismatches_and_never_overwrites() {
        let root = std::env::temp_dir().join(format!(
            "wiki-cli-native-census-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        private(&root);
        let source = root.join("source");
        let output = root.join("output");
        private(&source);
        private(&output);
        let expected = root.join("expected.ids");
        fs::write(&expected, b"1\n201\n").unwrap();
        let expected_sha = format!("{:x}", Sha256::digest(b"1\n201\n"));
        let names = [
            "enwiki-2026-10-01-p1p100.xml.bz2",
            "enwiki-2026-10-01-p201p300.xml.bz2",
        ];
        let pages = [
            "<mediawiki><siteinfo><dbname>enwiki</dbname></siteinfo><page><title>Physics</title><ns>0</ns><id>1</id><revision><id>2</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
            "<mediawiki><siteinfo><dbname>enwiki</dbname></siteinfo><page><title>Biology</title><ns>0</ns><id>201</id><revision><id>202</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
        ];
        let mut members = Vec::new();
        for (name, xml) in names.iter().zip(pages) {
            let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
            encoder.write_all(xml.as_bytes()).unwrap();
            let bytes = encoder.finish().unwrap();
            fs::write(source.join(name), &bytes).unwrap();
            members.push(bytes);
        }
        let sums = format!(
            "{:x}  {}\n{:x}  {}\n",
            Sha256::digest(&members[0]),
            names[0],
            Sha256::digest(&members[1]),
            names[1],
        );
        fs::write(source.join("SHA256SUMS"), &sums).unwrap();
        let inventory_sha = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let options = NativeDumpAudit {
            source_root: &source,
            project: "enwiki",
            generation: "2026-10-01",
            inventory_sha256: &inventory_sha,
            expected_path: &expected,
            expected_sha256: &expected_sha,
            namespace: 0,
            output_root: &output,
            output_name: "observed.ids",
            max_compressed_bytes: 4096,
            max_decoded_bytes: 4096,
        };
        assert_eq!(run(&options).unwrap(), 2);
        assert_eq!(fs::read(output.join("observed.ids")).unwrap(), b"1\n201\n");
        assert!(run(&options).is_err(), "never overwrite an existing output");
        assert!(run(&NativeDumpAudit { output_name: "nested/escape.ids", ..options }).is_err());
        assert!(run(&NativeDumpAudit { expected_sha256: &"0".repeat(64), output_name: "bad-pin.ids", ..options }).is_err());
        assert!(!output.join("bad-pin.ids").exists());
        fs::write(&expected, b"1\n202\n").unwrap();
        let changed_expected = format!("{:x}", Sha256::digest(b"1\n202\n"));
        assert!(run(&NativeDumpAudit { expected_sha256: &changed_expected, output_name: "wrong-ids.ids", ..options }).is_err());
        assert!(!output.join("wrong-ids.ids").exists());
        fs::write(&expected, b"1\n201\n").unwrap();
        fs::write(source.join(names[1]), b"corrupt member").unwrap();
        assert!(run(&NativeDumpAudit { output_name: "bad-member.ids", ..options }).is_err());
        assert!(!output.join("bad-member.ids").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::remove_file(source.join(names[1])).unwrap();
            symlink(source.join(names[0]), source.join(names[1])).unwrap();
            assert!(run(&NativeDumpAudit { output_name: "symlink.ids", ..options }).is_err());
            assert!(!output.join("symlink.ids").exists());
        }
        assert_eq!(fs::read_dir(&output).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
