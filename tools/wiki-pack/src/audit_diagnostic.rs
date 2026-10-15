//! Diagnostic-only source-record and independently pinned census comparison.
//! This does NOT authenticate Wikimedia publications or create offline packs.

use std::fs::{self, File, Metadata};
use std::io::{self, BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};
use wiki_source::public_dump_census_stream::CensusScope;
use wiki_source::public_dump_revision_jsonl::audit_revision_content_census;

const MAX_INPUT_BYTES: u64 = 2_000_000_000;

struct HashedReader<R> {
    inner: R,
    digest: Sha256,
}

impl<R: Read> Read for HashedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.digest.update(&buffer[..count]);
        Ok(count)
    }
}

fn open_input(path: &Path) -> Result<(File, Metadata), String> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let meta = fs::symlink_metadata(ancestor)
            .map_err(|error| format!("cannot inspect diagnostic path: {error}"))?;
        if meta.file_type().is_symlink() {
            return Err("diagnostic input path contains a symlink".into());
        }
    }
    let file = File::open(path)
        .map_err(|error| format!("cannot open diagnostic input: {error}"))?;
    let meta = file.metadata()
        .map_err(|error| format!("cannot stat diagnostic input: {error}"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_INPUT_BYTES {
        return Err("diagnostic input is not a bounded nonempty regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err("diagnostic input has multiple hard links".into());
        }
    }
    Ok((file, meta))
}

fn unchanged(file: &File, before: &Metadata) -> Result<bool, String> {
    let after = file.metadata()
        .map_err(|error| format!("cannot recheck input: {error}"))?;
    let stable = before.len() == after.len()
        && before.modified().ok() == after.modified().ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Ok(stable
            && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.nlink() == 1
            && after.nlink() == 1);
    }
    #[cfg(not(unix))]
    {
        Ok(stable)
    }
}

pub fn run(
    revision_path: &Path,
    census_path: &Path,
    project: &str,
    generation: &str,
    namespace: i32,
    expected_sha256: &str,
) -> Result<usize, String> {
    if expected_sha256.len() != 64
        || !expected_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("expected census digest must contain 64 hexadecimal characters".into());
    }
    let (mut records, record_meta) = open_input(revision_path)?;
    let (census, census_meta) = open_input(census_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if record_meta.dev() == census_meta.dev()
            && record_meta.ino() == census_meta.ino()
        {
            return Err("record and expected census inputs alias".into());
        }
    }
    let mut expected = BufReader::new(HashedReader {
        inner: census,
        digest: Sha256::new(),
    });
    let scope = CensusScope {
        project,
        generation,
        namespace,
    };
    let counts = audit_revision_content_census(
        &mut expected,
        BufReader::new(&mut records),
        scope,
    )
    .map_err(|error| format!("diagnostic census reconciliation failed: {error:?}"))?;
    let hashed = expected.into_inner();
    if format!("{:x}", hashed.digest.finalize()) != expected_sha256.to_ascii_lowercase() {
        return Err("expected census digest differs from consumed bytes".into());
    }
    if !unchanged(&records, &record_meta)?
        || !unchanged(&hashed.inner, &census_meta)?
    {
        return Err("diagnostic input changed during validation".into());
    }
    println!(
        "Matched {} diagnostic revision-content records to digest-pinned expected IDs",
        counts.observed_count
    );
    println!("Official publisher, independent census and generation scope: NOT AUTHENTICATED");
    println!("Rendered HTML, mandatory media and offline pack readiness: NOT VERIFIED");
    Ok(counts.observed_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn cli_diagnostic_gate_requires_matching_ids_and_expected_digest() {
        let root = std::env::temp_dir().join(format!(
            "wiki-source-cli-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let revisions = root.join("revisions.jsonl");
        let census = root.join("expected.ids");
        let row = serde_json::json!({
            "schema_version": 1,
            "project": "enwiki",
            "generation_id": "2026-10-01",
            "page_id": 1,
            "namespace": 0,
            "title": "München",
            "revision_id": 300,
            "timestamp": "2026-10-01T00:00:00Z",
            "redirect_target": null,
            "wikitext": "Sample",
            "wikitext_sha256": "95a2f1edb08a76120b82cec7c93ca477618639f04beac7d2aeba2dc039d43715"
        });
        fs::write(&revisions, format!("{row}\n")).unwrap();
        fs::write(&census, b"1\n").unwrap();
        let good_digest = format!("{:x}", Sha256::digest(b"1\n"));
        assert_eq!(
            run(&revisions, &census, "enwiki", "2026-10-01", 0, &good_digest),
            Ok(1)
        );
        assert!(run(&revisions, &census, "enwiki", "2026-10-01", 0, &"0".repeat(64)).is_err());
        assert!(run(&revisions, &census, "enwiki", "2026-10-01", 1, &good_digest).is_err());
        fs::write(&census, b"2\n").unwrap();
        let wrong_digest = format!("{:x}", Sha256::digest(b"2\n"));
        assert!(run(&revisions, &census, "enwiki", "2026-10-01", 0, &wrong_digest).is_err());
        fs::write(&census, b"1\n").unwrap();
        fs::write(&revisions, format!("{row}")).unwrap();
        assert!(run(&revisions, &census, "enwiki", "2026-10-01", 0, &good_digest).is_err());
        #[cfg(unix)]
        {
            let alias = root.join("symlink.jsonl");
            std::os::unix::fs::symlink(&revisions, &alias).unwrap();
            assert!(run(&alias, &census, "enwiki", "2026-10-01", 0, &good_digest).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
