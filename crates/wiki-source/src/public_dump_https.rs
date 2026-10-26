//! Host-only, certificate-verified HTTPS acquisition of official current-content
//! SHA256SUMS inventories. A caller-provided SHA-256 pin is not a publisher
//! signature or proof of independent release provenance.
//!
//! Only the exact known Wikimedia URL may be fetched: redirects, arbitrary
//! hosts/ports, user curl configuration, and unbounded response bodies are
//! forbidden. Android requires a separate platform transport.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

use crate::public_dump_origin::is_official_checksum_url;
use crate::public_dump_pinned::{
    parse_origin_checked_pinned_inventory, stage_pinned_member, PinnedInventory,
    PinnedInventoryError,
};
use crate::public_dump_stage::StageWriteError;
use crate::public_dump_verify::{verify_dump_member, DumpVerifyError};

/// A failed exec with ETXTBSY has not launched a process, so a bounded
/// retry is safe and does not duplicate an HTTP request. The same workaround
/// is used by the online article curl adapter on concurrent CI runners.
fn retry_executable_busy<T>(mut spawn: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut retries = 0;
    loop {
        match spawn() {
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && retries < 3 => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20 * retries));
            }
            result => return result,
        }
    }
}

pub const MAX_INVENTORY_BYTES: usize = 32 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;
/// Bound one resumed HTTPS transfer to 2 MiB of RAM, regardless of headers.
pub const MAX_RANGE_BYTES: usize = 2 * 1024 * 1024;

/// Curl's write-out must supply one unambiguous diagnostic marker. Never
/// allow a later line to override an earlier status or Content-Range: such
/// output is malformed, even if one of the values would otherwise pass.
fn unique_curl_marker<'a>(diagnostics: &'a str, prefix: &str) -> Option<&'a str> {
    let mut values = diagnostics
        .lines()
        .filter_map(|line| line.strip_prefix(prefix));
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedRange {
    pub total_bytes: u64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RangeFetchError {
    InvalidMember,
    InvalidRange,
    SpawnFailed,
    IoFailure,
    ResponseTooLarge,
    HttpStatus(u16),
    InvalidContentRange,
}

/// Require exactly one canonical RFC 7233 byte range for this request.
fn parse_exact_content_range(
    header: &str,
    requested_start: u64,
    requested_end: u64,
    byte_budget: u64,
    actual_size: usize,
) -> Result<u64, RangeFetchError> {
    let (unit, numbers) = header
        .split_once(' ')
        .ok_or(RangeFetchError::InvalidContentRange)?;
    if unit != "bytes" {
        return Err(RangeFetchError::InvalidContentRange);
    }
    let (range, total) = numbers
        .split_once('/')
        .ok_or(RangeFetchError::InvalidContentRange)?;
    let (start, end) = range
        .split_once('-')
        .ok_or(RangeFetchError::InvalidContentRange)?;
    // Parsing via u64 rules out wildcards, multiple ranges, negative offsets
    // and duplicate/injected Content-Range strings.
    let start = start
        .parse::<u64>()
        .map_err(|_| RangeFetchError::InvalidContentRange)?;
    let end = end
        .parse::<u64>()
        .map_err(|_| RangeFetchError::InvalidContentRange)?;
    let total = total
        .parse::<u64>()
        .map_err(|_| RangeFetchError::InvalidContentRange)?;
    if start != requested_start
        || end < start
        || end > requested_end
        || total == 0
        || total > byte_budget
        || end >= total
        || (end != requested_end && end != total - 1)
        || end - start + 1 != actual_size as u64
    {
        return Err(RangeFetchError::InvalidContentRange);
    }
    Ok(total)
}

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

/// Error when fetching a specific pinned compressed member. The member is
/// never published if the HTTPS subprocess or final checksum fails.
#[derive(Debug)]
pub enum MemberFetchError {
    InvalidMember,
    InvalidLimit,
    UnsafeRoot,
    SpawnFailed,
    Stage(StageWriteError),
    Existing(DumpVerifyError),
}

/// The entire HTTP process must exit successfully with status 200 before
/// this reader returns EOF. A failed HTTPS transfer becomes an I/O error
/// instead, so stage_pinned_member cannot publish a successful partial body.
struct MemberCurlStream {
    child: Child,
    stdout: ChildStdout,
    stderr_reader: Option<std::thread::JoinHandle<io::Result<Vec<u8>>>>,
    done: bool,
}

impl Read for MemberCurlStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        let n = self.stdout.read(buf)?;
        if n != 0 {
            return Ok(n);
        }
        self.done = true;
        let status = self.child.wait()?;
        let stderr_bytes = self
            .stderr_reader
            .take()
            .ok_or_else(|| io::Error::other("missing curl status reader"))?
            .join()
            .map_err(|_| io::Error::other("curl status reader panicked"))??;
        let diagnostics = String::from_utf8_lossy(&stderr_bytes);
        let http_status = unique_curl_marker(&diagnostics, "FW_DUMP_HTTP_CODE:")
            .and_then(|code| code.parse::<u16>().ok());
        if !status.success() || http_status != Some(200) {
            return Err(io::Error::other("verified member HTTPS request failed"));
        }
        Ok(0)
    }
}

impl Drop for MemberCurlStream {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}

fn drain_member_stderr(
    mut stderr: impl Read + Send + 'static,
) -> std::thread::JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut tail = VecDeque::with_capacity(MAX_DIAGNOSTIC_BYTES);
        let mut buffer = [0u8; 8192];
        loop {
            let count = stderr.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            tail.extend(&buffer[..count]);
            let excess = tail.len().saturating_sub(MAX_DIAGNOSTIC_BYTES);
            tail.drain(..excess);
        }
        Ok(tail.into_iter().collect())
    })
}

/// A complete private shard may be reused only after its full SHA-256 is
/// rechecked. Occupied but unsafe or corrupted paths never trigger downloads.
/// The caller must exclude untrusted concurrent writers from the staging tree.
pub fn verified_existing_member(
    inventory: &PinnedInventory,
    member_index: usize,
    root: &Path,
    max_bytes: u64,
) -> Result<Option<u64>, MemberFetchError> {
    let member = inventory
        .members()
        .get(member_index)
        .ok_or(MemberFetchError::InvalidMember)?;
    let relative = Path::new(&member.relative_path);
    let mut parent = root.to_path_buf();
    if let Some(directory) = relative.parent() {
        for component in directory.components() {
            parent.push(component);
            let metadata = match fs::symlink_metadata(&parent) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(MemberFetchError::UnsafeRoot),
            };
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(MemberFetchError::UnsafeRoot);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o022 != 0 {
                    return Err(MemberFetchError::UnsafeRoot);
                }
            }
        }
    }
    let target = root.join(relative);
    let inspected = match fs::symlink_metadata(&target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(MemberFetchError::UnsafeRoot),
    };
    if !inspected.file_type().is_file() || inspected.file_type().is_symlink() {
        return Err(MemberFetchError::UnsafeRoot);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if inspected.nlink() != 1 {
            return Err(MemberFetchError::UnsafeRoot);
        }
    }
    let mut file = File::open(&target).map_err(|_| MemberFetchError::UnsafeRoot)?;
    let opened = file.metadata().map_err(|_| MemberFetchError::UnsafeRoot)?;
    if !opened.is_file() || opened.len() != inspected.len() {
        return Err(MemberFetchError::UnsafeRoot);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != inspected.dev() || opened.ino() != inspected.ino() || opened.nlink() != 1
        {
            return Err(MemberFetchError::UnsafeRoot);
        }
    }
    let bytes =
        verify_dump_member(&mut file, member, max_bytes).map_err(MemberFetchError::Existing)?;
    let after = file.metadata().map_err(|_| MemberFetchError::UnsafeRoot)?;
    if after.len() != opened.len() {
        return Err(MemberFetchError::UnsafeRoot);
    }
    Ok(Some(bytes))
}

impl InventoryCurl {
    pub fn new() -> Self {
        Self {
            executable: "curl".into(),
        }
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
        if expected_sha256.len() != 64 || !expected_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(InventoryFetchError::InvalidDigest);
        }
        let raw = self.fetch_raw(&url, MAX_INVENTORY_BYTES)?;
        parse_origin_checked_pinned_inventory(&raw, project, date, &url, expected_sha256)
            .map_err(InventoryFetchError::Inventory)?;
        Ok(raw)
    }

    /// Stream exactly one member declared by the pinned inventory from its
    /// canonical official HTTPS URL into private, digest-verified staging.
    /// Source bytes are never published before curl exits with HTTP 200.
    /// Verified cached members are reused without HTTP. Partial downloads are
    /// not resumable, and this is not publisher provenance or a full dump.
    pub fn stage_verified_member(
        &self,
        inventory: &PinnedInventory,
        member_index: usize,
        root: &Path,
        max_bytes: u64,
    ) -> Result<u64, MemberFetchError> {
        if max_bytes == 0 || max_bytes > (1_u64 << 40) {
            return Err(MemberFetchError::InvalidLimit);
        }
        let url = inventory
            .official_member_url(member_index)
            .ok_or(MemberFetchError::InvalidMember)?;
        // Reject path aliases and untrusted staging roots before network I/O.
        for ancestor in root.ancestors() {
            if ancestor.as_os_str().is_empty() {
                continue;
            }
            let metadata =
                fs::symlink_metadata(ancestor).map_err(|_| MemberFetchError::UnsafeRoot)?;
            if metadata.file_type().is_symlink() {
                return Err(MemberFetchError::UnsafeRoot);
            }
        }
        let root_meta = fs::symlink_metadata(root).map_err(|_| MemberFetchError::UnsafeRoot)?;
        if !root_meta.is_dir() {
            return Err(MemberFetchError::UnsafeRoot);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if root_meta.permissions().mode() & 0o077 != 0 {
                return Err(MemberFetchError::UnsafeRoot);
            }
        }
        if let Some(bytes) = verified_existing_member(inventory, member_index, root, max_bytes)? {
            return Ok(bytes);
        }
        // -q first disables .curlrc; never use -k, -L or user-supplied URLs.
        let mut command = Command::new(&self.executable);
        command
            .args([
                "-q",
                "--silent",
                "--show-error",
                "--globoff",
                "--user-agent",
                "FoundationWikipedia/0.1 (dump member)",
                "--header",
                "Accept: application/octet-stream",
                "--proto",
                "=https",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "10",
                "--max-time",
                "3600",
                "--max-filesize",
                &max_bytes.to_string(),
                "--output",
                "-",
                "--write-out",
                "%{stderr}\nFW_DUMP_HTTP_CODE:%{http_code}\n",
                "--",
                &url,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = retry_executable_busy(|| command.spawn()).map_err(|error| {
            eprintln!(
                "wiki-source public dump curl spawn failed: kind={:?}, raw_os_error={:?}",
                error.kind(),
                error.raw_os_error()
            );
            MemberFetchError::SpawnFailed
        })?;
        let stdout = child.stdout.take().ok_or(MemberFetchError::SpawnFailed)?;
        let stderr = child.stderr.take().ok_or(MemberFetchError::SpawnFailed)?;
        let mut stream = MemberCurlStream {
            child,
            stdout,
            stderr_reader: Some(drain_member_stderr(stderr)),
            done: false,
        };
        stage_pinned_member(root, inventory, member_index, &mut stream, max_bytes)
            .map_err(MemberFetchError::Stage)
    }

    /// Fetch one bounded, explicit HTTP byte range for a checksum-declared
    /// member. Curl must report HTTP 206 and the exact Content-Range. The
    /// returned bytes are NOT individually authenticated: the full shard's
    /// independently pinned SHA-256 must still be verified before publishing.
    /// Requires curl 7.84+ for the %header{content-range} write-out variable.
    pub fn fetch_member_range(
        &self,
        inventory: &PinnedInventory,
        member_index: usize,
        start: u64,
        chunk_bytes: usize,
        byte_budget: u64,
    ) -> Result<VerifiedRange, RangeFetchError> {
        if chunk_bytes == 0
            || chunk_bytes > MAX_RANGE_BYTES
            || byte_budget == 0
            || byte_budget > (1_u64 << 40)
            || start >= byte_budget
        {
            return Err(RangeFetchError::InvalidRange);
        }
        let url = inventory
            .official_member_url(member_index)
            .ok_or(RangeFetchError::InvalidMember)?;
        let end = start
            .checked_add(chunk_bytes as u64 - 1)
            .filter(|end| *end < byte_budget)
            .ok_or(RangeFetchError::InvalidRange)?;
        let range = format!("{start}-{end}");
        let mut command = Command::new(&self.executable);
        command
            .args([
                "-q",
                "--silent",
                "--show-error",
                "--globoff",
                "--user-agent",
                "FoundationWikipedia/0.1 (resumable dump member)",
                "--header",
                "Accept: application/octet-stream",
                "--header",
                "Accept-Encoding: identity",
                "--proto",
                "=https",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "10",
                "--max-time",
                "180",
                "--max-filesize",
                &chunk_bytes.to_string(),
                "--range",
                &range,
                "--output",
                "-",
                "--write-out",
                "%{stderr}\\nFW_DUMP_HTTP_CODE:%{http_code}\\nFW_DUMP_CONTENT_RANGE:%header{content-range}\\n",
                "--",
                &url,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = retry_executable_busy(|| command.spawn()).map_err(|error| {
            eprintln!(
                "wiki-source public dump curl spawn failed: kind={:?}, raw_os_error={:?}",
                error.kind(),
                error.raw_os_error()
            );
            RangeFetchError::SpawnFailed
        })?;
        let stderr = child.stderr.take().ok_or(RangeFetchError::IoFailure)?;
        let reader = drain_member_stderr(stderr);
        let mut stdout = child.stdout.take().ok_or(RangeFetchError::IoFailure)?;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = match stdout.read(&mut buffer) {
                Ok(n) => n,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(RangeFetchError::IoFailure);
                }
            };
            if n == 0 {
                break;
            }
            if n > chunk_bytes.saturating_sub(bytes.len()) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(RangeFetchError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&buffer[..n]);
        }
        drop(stdout);
        let status = child.wait().map_err(|_| RangeFetchError::IoFailure)?;
        let diagnostics = reader
            .join()
            .map_err(|_| RangeFetchError::IoFailure)?
            .map_err(|_| RangeFetchError::IoFailure)?;
        let diagnostics = String::from_utf8_lossy(&diagnostics);
        let http = unique_curl_marker(&diagnostics, "FW_DUMP_HTTP_CODE:")
            .and_then(|code| code.parse::<u16>().ok());
        if let Some(code) = http {
            if code != 206 && code != 0 {
                return Err(RangeFetchError::HttpStatus(code));
            }
        }
        if !status.success() || http != Some(206) {
            return Err(RangeFetchError::InvalidContentRange);
        }
        let header = unique_curl_marker(&diagnostics, "FW_DUMP_CONTENT_RANGE:")
            .ok_or(RangeFetchError::InvalidContentRange)?;
        let total = parse_exact_content_range(header, start, end, byte_budget, bytes.len())?;
        Ok(VerifiedRange {
            total_bytes: total,
            bytes,
        })
    }

    fn fetch_raw(&self, url: &str, max_bytes: usize) -> Result<Vec<u8>, InventoryFetchError> {
        if max_bytes == 0 || max_bytes > MAX_INVENTORY_BYTES {
            return Err(InventoryFetchError::InvalidLimit);
        }
        // -q first suppresses ~/.curlrc. Do not use -L, -k or --insecure.
        // Curl verifies the TLS certificate with the installed system CA roots.
        // Limit stdout independently of Content-Length / --max-filesize.
        let mut command = Command::new(&self.executable);
        command
            .args([
                "-q",
                "--silent",
                "--show-error",
                "--globoff",
                "--user-agent",
                "FoundationWikipedia/0.1 (dump inventory)",
                "--header",
                "Accept: text/plain",
                "--proto",
                "=https",
                "--max-redirs",
                "0",
                "--connect-timeout",
                "10",
                "--max-time",
                "60",
                "--max-filesize",
                &max_bytes.to_string(),
                "--output",
                "-",
                "--write-out",
                "%{stderr}\nFW_DUMP_HTTP_CODE:%{http_code}\n",
                "--",
                url,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = retry_executable_busy(|| command.spawn()).map_err(|error| {
            eprintln!(
                "wiki-source public dump curl spawn failed: kind={:?}, raw_os_error={:?}",
                error.kind(),
                error.raw_os_error()
            );
            InventoryFetchError::SpawnFailed
        })?;

        let mut stderr = child.stderr.take().ok_or(InventoryFetchError::IoFailure)?;
        let drain_stderr = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut tail = VecDeque::with_capacity(MAX_DIAGNOSTIC_BYTES);
            let mut buffer = [0u8; 8192];
            loop {
                let n = stderr.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
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
            if n == 0 {
                break;
            }
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
        let http_status = unique_curl_marker(&diagnostics, "FW_DUMP_HTTP_CODE:")
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
        let path = std::env::temp_dir().join(format!(
            "wiki-dump-inventory-curl-{}-{id}",
            std::process::id()
        ));
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        (
            InventoryCurl {
                executable: path.clone(),
            },
            path,
        )
    }

    fn inventory_bytes() -> String {
        format!("{}  enwiki-2026-10-01-p1p100.xml.bz2\n", "a".repeat(64),)
    }

    #[test]
    fn busy_executable_retries_without_retrying_permission_errors() {
        let mut attempts = 0;
        let result = retry_executable_busy(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::ExecutableFileBusy))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);

        let mut denied_attempts = 0;
        let denied: io::Result<()> = retry_executable_busy(|| {
            denied_attempts += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(denied.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(denied_attempts, 1);
    }

    #[test]
    fn fetches_exact_official_inventory_with_bounded_no_redirect_https_args() {
        let raw = inventory_bytes();
        let sha256 = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let args_path = std::env::temp_dir().join(format!(
            "wiki-dump-curl-args-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let script = format!(
            "printf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\nprintf 'FW_DUMP_HTTP_CODE:200\\n' >&2",
            args_path.display(),
            raw
        );
        let (client, executable) = fake_curl(&script);
        assert_eq!(
            client
                .fetch_verified_inventory("enwiki", "2026-10-01", &sha256)
                .unwrap(),
            raw.as_bytes()
        );
        let args = fs::read_to_string(&args_path).unwrap();
        let words: Vec<_> = args.lines().collect();
        assert_eq!(words.first(), Some(&"-q"));
        assert!(words.windows(2).any(|pair| pair == ["--proto", "=https"]));
        assert!(words.windows(2).any(|pair| pair == ["--max-redirs", "0"]));
        assert!(words
            .windows(2)
            .any(|pair| pair == ["--max-filesize", "33554432"]));
        assert!(words.windows(2).any(|pair| pair == ["--", "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS"]));
        assert!(!words
            .iter()
            .any(|word| ["-L", "-k", "--insecure", "--location"].contains(word)));
        fs::remove_file(&args_path).unwrap();
        fs::remove_file(executable).unwrap();
    }

    #[test]
    fn invalid_identity_digest_and_mismatched_inventory_fail_closed() {
        let raw = inventory_bytes();
        let sha256 = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let (client, executable) = fake_curl("exit 99");
        for (project, date, digest, error) in [
            (
                "bad/wiki",
                "2026-10-01",
                sha256.as_str(),
                InventoryFetchError::InvalidIdentity,
            ),
            (
                "enwiki",
                "2026-02-30",
                sha256.as_str(),
                InventoryFetchError::InvalidIdentity,
            ),
            (
                "enwiki",
                "2026-10-01",
                "bad",
                InventoryFetchError::InvalidDigest,
            ),
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
            Err(InventoryFetchError::Inventory(
                PinnedInventoryError::DigestMismatch
            ))
        ));
        assert!(matches!(
            client.fetch_verified_inventory("frwiki", "2026-10-01", &sha256),
            Err(InventoryFetchError::Inventory(
                PinnedInventoryError::WrongGeneration
            ))
        ));
        fs::remove_file(executable).unwrap();
    }

    #[test]
    fn member_stream_publishes_only_successful_digest_pinned_http200() {
        let body = b"compressed fixture member bytes";
        let name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let sums = format!("{:x}  {name}\n", Sha256::digest(body));
        let pin = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let inventory = parse_origin_checked_pinned_inventory(
            sums.as_bytes(),
            "enwiki",
            "2026-10-01",
            "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            &pin,
        ).unwrap();
        let root = std::env::temp_dir().join(format!(
            "wiki-remote-member-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();

        let (client, exe) = fake_curl(
            "printf 'compressed fixture member bytes'; printf 'FW_DUMP_HTTP_CODE:200\n' >&2",
        );
        assert_eq!(
            client
                .stage_verified_member(&inventory, 0, &root, 256)
                .unwrap(),
            body.len() as u64
        );
        assert_eq!(fs::read(root.join(name)).unwrap(), body);
        fs::remove_file(exe).unwrap();

        // Valid cache hits do not spawn curl. Corrupted occupied paths cannot
        // be silently overwritten, even when a download would be possible.
        let (offline_client, offline_exe) = fake_curl("exit 99");
        assert_eq!(
            offline_client
                .stage_verified_member(&inventory, 0, &root, 256)
                .unwrap(),
            body.len() as u64
        );
        fs::write(root.join(name), b"corrupted").unwrap();
        assert!(matches!(
            offline_client.stage_verified_member(&inventory, 0, &root, 256),
            Err(MemberFetchError::Existing(DumpVerifyError::DigestMismatch))
        ));
        assert_eq!(fs::read(root.join(name)).unwrap(), b"corrupted");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = root.with_extension("outside");
            fs::write(&outside, body).unwrap();
            fs::remove_file(root.join(name)).unwrap();
            symlink(&outside, root.join(name)).unwrap();
            assert!(matches!(
                offline_client.stage_verified_member(&inventory, 0, &root, 256),
                Err(MemberFetchError::UnsafeRoot)
            ));
            fs::remove_file(root.join(name)).unwrap();
            fs::hard_link(&outside, root.join(name)).unwrap();
            assert!(matches!(
                offline_client.stage_verified_member(&inventory, 0, &root, 256),
                Err(MemberFetchError::UnsafeRoot)
            ));
            fs::remove_file(outside).unwrap();
        }
        fs::remove_file(offline_exe).unwrap();
        fs::remove_file(root.join(name)).unwrap();
        for (label, script) in [
            ("redirect", "printf 'compressed fixture member bytes'; printf 'FW_DUMP_HTTP_CODE:302\n' >&2"),
            ("failed", "printf 'compressed fixture member bytes'; printf 'FW_DUMP_HTTP_CODE:200\n' >&2; exit 2"),
            ("no-status", "printf 'compressed fixture member bytes'"),
            ("bad-checksum", "printf 'tampered'; printf 'FW_DUMP_HTTP_CODE:200\n' >&2"),
        ] {
            let (client, exe) = fake_curl(script);
            let output = client.stage_verified_member(&inventory, 0, &root, 256);
            assert!(output.is_err(), "{label}: {output:?}");
            assert!(!root.join(name).exists(), "{label}");
            assert_eq!(fs::read_dir(&root).unwrap().count(), 0, "{label}");
            fs::remove_file(exe).unwrap();
        }

        let (client, exe) = fake_curl("printf '123456789'; printf 'FW_DUMP_HTTP_CODE:200\n' >&2");
        assert!(matches!(
            client.stage_verified_member(&inventory, 0, &root, 8),
            Err(MemberFetchError::Stage(StageWriteError::TooLarge))
        ));
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        assert!(matches!(
            client.stage_verified_member(&inventory, 1, &root, 256),
            Err(MemberFetchError::InvalidMember)
        ));
        fs::remove_file(exe).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn member_ranges_require_exact_http206_content_range_and_bounded_body() {
        let raw = inventory_bytes();
        let sha = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let inventory = parse_origin_checked_pinned_inventory(
            raw.as_bytes(),
            "enwiki",
            "2026-10-01",
            "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            &sha,
        )
        .unwrap();
        let (client, exe) = fake_curl(
            "printf 'abcd'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\\n' >&2",
        );
        let response = client.fetch_member_range(&inventory, 0, 0, 4, 100).unwrap();
        assert_eq!(response.bytes, b"abcd");
        assert_eq!(response.total_bytes, 6);
        fs::remove_file(exe).unwrap();

        for (label, script) in [
            ("ignored range", "printf 'abcd'; printf 'FW_DUMP_HTTP_CODE:200\\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\\n' >&2"),
            ("wrong start", "printf 'abcd'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 1-4/6\\n' >&2"),
            ("wrong size", "printf 'abc'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\\n' >&2"),
            ("wrong total", "printf 'abcd'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 0-3/999\\n' >&2"),
            ("invalid suffix", "printf 'abcd'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 0-3/*\\n' >&2"),
            ("redirect", "printf 'abcd'; printf 'FW_DUMP_HTTP_CODE:302\\n' >&2"),
            ("early short", "printf 'ab'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 0-1/6\\n' >&2"),
            ("missing headers", "printf 'abcd'"),
        ] {
            let (client, exe) = fake_curl(script);
            let result = client.fetch_member_range(&inventory, 0, 0, 4, 100);
            assert!(result.is_err(), "{label}: {result:?}");
            fs::remove_file(exe).unwrap();
        }

        let (client, exe) = fake_curl(
            "printf 'abcdef'; printf 'FW_DUMP_HTTP_CODE:206\\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\\n' >&2",
        );
        assert_eq!(
            client.fetch_member_range(&inventory, 0, 0, 4, 100),
            Err(RangeFetchError::ResponseTooLarge)
        );
        assert_eq!(
            client.fetch_member_range(&inventory, 2, 0, 4, 100),
            Err(RangeFetchError::InvalidMember)
        );
        assert_eq!(
            client.fetch_member_range(&inventory, 0, 100, 4, 100),
            Err(RangeFetchError::InvalidRange)
        );
        fs::remove_file(exe).unwrap();
    }

    #[test]
    fn refuses_redirect_http_errors_missing_status_and_oversized_stdout() {
        let raw = inventory_bytes();
        let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
        for (script, expected) in [
            (
                "printf 'redirect'; printf 'FW_DUMP_HTTP_CODE:302\\n' >&2",
                InventoryFetchError::HttpStatus(302),
            ),
            (
                "printf 'missing'; printf 'FW_DUMP_HTTP_CODE:404\\n' >&2",
                InventoryFetchError::HttpStatus(404),
            ),
            (
                "printf 'body without status'",
                InventoryFetchError::ProcessFailed,
            ),
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

    #[test]
    fn curl_diagnostic_markers_must_be_unique_for_inventory_range_and_members() {
        let raw = inventory_bytes();
        let pin = format!("{:x}", Sha256::digest(raw.as_bytes()));
        for diagnostics in [
            "FW_DUMP_HTTP_CODE:200\nFW_DUMP_HTTP_CODE:200\n",
            "FW_DUMP_HTTP_CODE:404\nFW_DUMP_HTTP_CODE:200\n",
            "FW_DUMP_HTTP_CODE:invalid\nFW_DUMP_HTTP_CODE:200\n",
        ] {
            let script = format!("printf '%s' '{}'; printf '{}' >&2", raw, diagnostics);
            let (client, executable) = fake_curl(&script);
            assert_eq!(
                client.fetch_verified_inventory("enwiki", "2026-10-01", &pin),
                Err(InventoryFetchError::ProcessFailed)
            );
            fs::remove_file(executable).unwrap();
        }

        let inventory = parse_origin_checked_pinned_inventory(
            raw.as_bytes(),
            "enwiki",
            "2026-10-01",
            "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            &pin,
        )
        .unwrap();
        for diagnostics in [
            "FW_DUMP_HTTP_CODE:206\nFW_DUMP_HTTP_CODE:206\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\n",
            "FW_DUMP_HTTP_CODE:206\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\nFW_DUMP_CONTENT_RANGE:bytes 0-3/6\n",
        ] {
            let script = format!("printf 'abcd'; printf '{}' >&2", diagnostics);
            let (client, executable) = fake_curl(&script);
            assert_eq!(
                client.fetch_member_range(&inventory, 0, 0, 4, 100),
                Err(RangeFetchError::InvalidContentRange)
            );
            fs::remove_file(executable).unwrap();
        }

        let body = b"member-bytes";
        let member_name = "enwiki-2026-10-01-p1p100.xml.bz2";
        let sums = format!("{:x}  {member_name}\n", Sha256::digest(body));
        let member_pin = format!("{:x}", Sha256::digest(sums.as_bytes()));
        let member_inventory = parse_origin_checked_pinned_inventory(
            sums.as_bytes(),
            "enwiki",
            "2026-10-01",
            "https://dumps.wikimedia.org/other/mediawiki_content_current/enwiki/2026-10-01/xml/bzip2/SHA256SUMS",
            &member_pin,
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "wiki-duplicate-status-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let (client, executable) = fake_curl(
            "printf 'member-bytes'; printf 'FW_DUMP_HTTP_CODE:200\nFW_DUMP_HTTP_CODE:200\n' >&2",
        );
        assert!(client
            .stage_verified_member(&member_inventory, 0, &root, 256)
            .is_err());
        assert!(!root.join(member_name).exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_file(executable).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
