//! Host-only, certificate-verified HTTPS acquisition of official current-content
//! SHA256SUMS inventories. A caller-provided SHA-256 pin is not a publisher
//! signature or proof of independent release provenance.
//!
//! Only the exact known Wikimedia URL may be fetched: redirects, arbitrary
//! hosts/ports, user curl configuration, and unbounded response bodies are
//! forbidden. Android requires a separate platform transport.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::public_dump_origin::is_official_checksum_url;
use crate::public_dump_pinned::{
    parse_origin_checked_pinned_inventory, PinnedInventoryError,
};

pub const MAX_INVENTORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum InventoryFetchError {
    InvalidIdentity,
    InvalidDigest,
    InvalidLimit,
    SpawnFailed,
    IoFailure,
    ResponseTooLarge,
    HttpStatus(u16),
    ProcessFailed,
    Inventory(PinnedInventoryError),
}

/// Explicit executable path is a test seam, not part of public CLI input.
/// Production uses the system curl and system certificate trust store.
pub struct InventoryCurl {
    executable: PathBuf,
}

impl Default for InventoryCurl {
    fn default() -> Self {
        Self::new()
    }
}

impl InventoryCurl {
    pub fn new() -> Self {
        Self { executable: "curl".into() }
    }

    /// Return the exact raw publication bytes only if certificate-verified
    /// HTTPS succeeds and the independently supplied digest, generation and
    /// checksum inventory structure all match. Never fetch arbitrary URLs.
    pub fn fetch_verified_inventory(
        &self,
        project: &str,
        date: &str,
        expected_sha256: &str,
    ) -> Result<Vec<u8>, InventoryFetchError> {
        let url = format!(
            "https://dumps.wikimedia.org/other/mediawiki_content_current/{project}/{date}/xml/bzip2/SHA256SUMS"
        );
        if !is_official_checksum_url(&url, project, date) {
            return Err(InventoryFetchError::InvalidIdentity);
        }
        if expected_sha256.len() != 64
            || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(InventoryFetchError::InvalidDigest);
        }
        let raw = self.fetch_raw(&url, MAX_INVENTORY_BYTES)?;
        parse_origin_checked_pinned_inventory(
            &raw, project, date, &url, expected_sha256,
        )
        .map_err(InventoryFetchError::Inventory)?;
        Ok(raw)
    }

    fn fetch_raw(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, InventoryFetchError> {
        if max_bytes == 0 || max_bytes > MAX_INVENTORY_BYTES {
            return Err(InventoryFetchError::InvalidLimit);
        }
        // -q first suppresses ~/.curlrc. Do not use -L, -k or --insecure.
        // Curl verifies the TLS certificate with the installed system CA roots.
        // Limit stdout independently of Content-Length / --max-filesize.
        let mut child = Command::new(&self.executable)
            .args([
                "-q", "--silent", "--show-error", "--globoff",
                "--user-agent", "FoundationWikipedia/0.1 (dump inventory)",
                "--header", "Accept: text/plain",
                "--proto", "=https",
                "--max-redirs", "0",
                "--connect-timeout", "10",
                "--max-time", "60",
                "--max-filesize", &max_bytes.to_string(),
                "--output", "-",
                "--write-out", "%{stderr}\nFW_DUMP_HTTP_CODE:%{http_code}\n",
                "--", url,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| InventoryFetchError::SpawnFailed)?;

        let mut stderr = child.stderr.take().ok_or(InventoryFetchError::IoFailure)?;
        let drain_stderr = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut tail = VecDeque::with_capacity(MAX_DIAGNOSTIC_BYTES);
            let mut buffer = [0u8; 8192];
            loop {
                let n = stderr.read(&mut buffer)?;
                if n == 0 { break; }
                tail.extend(&buffer[..n]);
                let excess = tail.len().saturating_sub(MAX_DIAGNOSTIC_BYTES);
                tail.drain(..excess);
            }
            Ok(tail.into_iter().collect())
        });

        let mut stdout = child.stdout.take().ok_or(InventoryFetchError::IoFailure)?;
        let mut raw = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = stdout.read(&mut buffer);
            let n = match read {
                Ok(n) => n,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = drain_stderr.join();
                    return Err(InventoryFetchError::IoFailure);
                }
            };
            if n == 0 { break; }
            if n > max_bytes.saturating_sub(raw.len()) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = drain_stderr.join();
                return Err(InventoryFetchError::ResponseTooLarge);
            }
            raw.extend_from_slice(&buffer[..n]);
        }
        drop(stdout);
        let status = child.wait().map_err(|_| InventoryFetchError::IoFailure)?;
        let stderr_bytes = drain_stderr
            .join()
            .map_err(|_| InventoryFetchError::IoFailure)?
            .map_err(|_| InventoryFetchError::IoFailure)?;
        let diagnostics = String::from_utf8_lossy(&stderr_bytes);
        let http_status = diagnostics
            .lines()
            .filter_map(|line| line.strip_prefix("FW_DUMP_HTTP_CODE:"))
            .last()
            .and_then(|code| code.parse::<u16>().ok());
        if let Some(code) = http_status {
            if code != 200 && code != 0 {
                return Err(InventoryFetchError::HttpStatus(code));
            }
        }
        if !status.success() || http_status != Some(200) {
            return Err(InventoryFetchError::ProcessFailed);
        }
        Ok(raw)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fake_curl(script: &str) -> (InventoryCurl, PathBuf) {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join(format!("wiki-dump-inventory-curl-{}-{id}", std::process::id()));
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        (InventoryCurl { executable: path.clone() }, path)
    }

    fn inventory_bytes() -> String {
        format!(
            "{}  enwiki-2026-10-01-p1p100.xml.bz2\n",
            "a".repeat(64),
        )
    }

    #[test]
    fn fetches_exact_official_inventory_with_bounded_no_redirect_https_args() {
        let raw = inventory_bytes();
        let sha256 = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let args_path = std::env::temp_dir().join(format!(
            "wiki-dump-curl-args-{}-{}", std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let script = format!(
            "printf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\nprintf 'FW_DUMP_HTTP_CODE:200\\n' >&2",
            args_path.display(), raw
        );
        let (client, executable) = fake_curl(&script);
        assert_eq!(
            client.fetch_verified_inventory("enwiki", "2026-10-01", &sha256).unwrap(),
            raw.as_bytes()
        );
        let args = fs::read_to_string(&args_path).unwrap();
        let words: Vec<_> = args.lines().collect();
        assert_eq!(words.first(), Some(&"-q"));
        assert!(words.windows(2).any(|pair| pair == ["--proto", "=https"]));
        assert!(words.windows(2).any(|pair| pair == ["--max-redirs", "0"]));
        assert!(words.windows(2).any(|pair| pair == ["--max-filesize", "33554432"]));
        assert!(words.windows(2).any(|pair| pair == ["--", "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS"]));
        assert!(!words.iter().any(|word| ["-L", "-k", "--insecure", "--location"].contains(word)));
        fs::remove_file(&args_path).unwrap();
        fs::remove_file(executable).unwrap();
    }

    #[test]
    fn invalid_identity_digest_and_mismatched_inventory_fail_closed() {
        let raw = inventory_bytes();
        let sha256 = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let (client, executable) = fake_curl("exit 99");
        for (project, date, digest, error) in [
            ("bad/wiki", "2026-10-01", sha256.as_str(), InventoryFetchError::InvalidIdentity),
            ("enwiki", "2026-02-30", sha256.as_str(), InventoryFetchError::InvalidIdentity),
            ("enwiki", "2026-10-01", "bad", InventoryFetchError::InvalidDigest),
        ] {
            assert_eq!(
                client.fetch_verified_inventory(project, date, digest),
                Err(error)
            );
        }
        fs::remove_file(executable).unwrap();

        let script = format!(
            "printf '%s' '{}'\nprintf 'FW_DUMP_HTTP_CODE:200\\n' >&2",
            raw
        );
        let (client, executable) = fake_curl(&script);
        assert!(matches!(
            client.fetch_verified_inventory("enwiki", "2026-10-01", &"0".repeat(64)),
            Err(InventoryFetchError::Inventory(PinnedInventoryError::DigestMismatch))
        ));
        assert!(matches!(
            client.fetch_verified_inventory("frwiki", "2026-10-01", &sha256),
            Err(InventoryFetchError::Inventory(PinnedInventoryError::WrongGeneration))
        ));
        fs::remove_file(executable).unwrap();
    }

    #[test]
    fn refuses_redirect_http_errors_missing_status_and_oversized_stdout() {
        let raw = inventory_bytes();
        let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
        for (script, expected) in [
            ("printf 'redirect'; printf 'FW_DUMP_HTTP_CODE:302\\n' >&2", InventoryFetchError::HttpStatus(302)),
            ("printf 'missing'; printf 'FW_DUMP_HTTP_CODE:404\\n' >&2", InventoryFetchError::HttpStatus(404)),
            ("printf 'body without status'", InventoryFetchError::ProcessFailed),
        ] {
            let (client, executable) = fake_curl(script);
            assert_eq!(
                client.fetch_verified_inventory("enwiki", "2026-10-01", &pin),
                Err(expected),
            );
            fs::remove_file(executable).unwrap();
        }
        let (client, executable) =
            fake_curl("printf '123456789'; printf 'FW_DUMP_HTTP_CODE:200\\n' >&2");
        assert_eq!(
            client.fetch_raw(
                "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS", 8
            ),
            Err(InventoryFetchError::ResponseTooLarge),
        );
        fs::remove_file(executable).unwrap();
    }
}
