//! Desktop/host-only, bounded HTTPS adapter for Wikimedia Action API reads.
//!
//! Uses the system curl executable with certificate verification enabled,
//! no redirect following, fixed deadlines, and a streaming response cap.
//! Android needs its own platform HTTP transport. A successful response
//! authenticates the TLS peer, NOT offline media completeness or pack trust.

use std::collections::VecDeque;
use std::io::Read;
use std::process::{Command, Stdio};

use url::Url;

use crate::online_fetch::MediaWikiTransport;
use crate::online_requests::WikipediaRequests;

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurlTransportError {
    InvalidRequest,
    InvalidLimit,
    SpawnFailed,
    IoFailure,
    ResponseTooLarge,
    HttpStatus(u16),
    ProcessFailed,
}

/// Host-only transport; no Android curl executable is assumed.
/// The curl binary and system trust store are deployment prerequisites.
#[derive(Clone, Debug)]
pub struct CurlTransport {
    executable: std::path::PathBuf,
}

impl Default for CurlTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl CurlTransport {
    pub fn new() -> Self {
        Self {
            executable: "curl".into(),
        }
    }

    fn checked_url(url: &str) -> Result<(), CurlTransportError> {
        let parsed = Url::parse(url).map_err(|_| CurlTransportError::InvalidRequest)?;
        let language = parsed
            .host_str()
            .and_then(|host| host.strip_suffix(".wikipedia.org"))
            .ok_or(CurlTransportError::InvalidRequest)?;
        let requests = WikipediaRequests::new(&format!("{language}wiki"))
            .map_err(|_| CurlTransportError::InvalidRequest)?;
        if !requests.accepts_transport_url(url) {
            return Err(CurlTransportError::InvalidRequest);
        }
        Ok(())
    }
}

impl MediaWikiTransport for CurlTransport {
    type Error = CurlTransportError;

    fn get(&mut self, url: &str, max_bytes: usize) -> Result<Vec<u8>, Self::Error> {
        Self::checked_url(url)?;
        if max_bytes == 0 || max_bytes > MAX_RESPONSE_BYTES {
            return Err(CurlTransportError::InvalidLimit);
        }

        // -q must be the first curl option to disable user .curlrc settings.
        // Never pass -L/--location, -k/--insecure, or arbitrary headers.
        // Even when Content-Length is absent, stdout is capped while reading.
        let mut child = Command::new(&self.executable)
            .args([
                "-q",
                "--silent",
                "--show-error",
                "--globoff",
                "--user-agent",
                "FoundationWikipedia/0.1 (+https://github.com/ekkus93/foundation-wikipedia)",
                "--header",
                "Accept: application/json",
                "--proto",
                "=https",
                "--max-time",
                "15",
                "--connect-timeout",
                "5",
                "--max-redirs",
                "0",
                "--max-filesize",
                &max_bytes.to_string(),
                "--output",
                "-",
                "--write-out",
                "%{stderr}FW_HTTP_CODE:%{http_code}\\n",
                "--",
                url,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| CurlTransportError::SpawnFailed)?;

        // Read both subprocess pipes concurrently to avoid stderr pipe deadlock.
        // Keep only the last 64 KiB, including the final HTTP status marker.
        let mut stderr = child.stderr.take().ok_or(CurlTransportError::IoFailure)?;
        let stderr_reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
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
        });

        let mut stdout = child.stdout.take().ok_or(CurlTransportError::IoFailure)?;
        let mut body = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let count = match stdout.read(&mut buffer) {
                Ok(count) => count,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stderr_reader.join();
                    return Err(CurlTransportError::IoFailure);
                }
            };
            if count == 0 {
                break;
            }
            if count > max_bytes.saturating_sub(body.len()) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stderr_reader.join();
                return Err(CurlTransportError::ResponseTooLarge);
            }
            body.extend_from_slice(&buffer[..count]);
        }
        drop(stdout);

        let status = child.wait().map_err(|_| CurlTransportError::IoFailure)?;
        let stderr_bytes = stderr_reader
            .join()
            .map_err(|_| CurlTransportError::IoFailure)?
            .map_err(|_| CurlTransportError::IoFailure)?;
        let diagnostics = String::from_utf8_lossy(&stderr_bytes);
        let http_status = diagnostics
            .lines()
            .filter_map(|line| line.strip_prefix("FW_HTTP_CODE:"))
            .last()
            .and_then(|code| code.parse::<u16>().ok());
        if let Some(code) = http_status {
            if code != 200 && code != 0 {
                return Err(CurlTransportError::HttpStatus(code));
            }
        }
        if !status.success() || http_status != Some(200) {
            return Err(CurlTransportError::ProcessFailed);
        }
        Ok(body)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_ID: AtomicUsize = AtomicUsize::new(0);

    fn fake_curl(script: &str) -> (CurlTransport, std::path::PathBuf) {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("foundation-wiki-curl-{}-{id}", std::process::id()));
        fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        (
            CurlTransport {
                executable: path.clone(),
            },
            path,
        )
    }

    #[test]
    fn rejects_untrusted_urls_before_spawning_any_process() {
        let (mut client, path) = fake_curl("exit 99");
        for url in [
            "http://en.wikipedia.org/w/api.php",
            "https://evil.example/w/api.php",
            "https://en.wikipedia.org/w/api.php?action=delete",
            "https://en.wikipedia.org:444/w/api.php?action=query",
            "https://en.wikipedia.org/w/api.php?action=query&action=parse",
        ] {
            assert_eq!(
                client.get(url, 1024),
                Err(CurlTransportError::InvalidRequest),
                "{url}"
            );
        }
        let valid = WikipediaRequests::new("enwiki")
            .unwrap()
            .search("Physics", 2)
            .unwrap();
        assert_eq!(client.get(&valid, 0), Err(CurlTransportError::InvalidLimit));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn accepts_exact_canonical_request_and_rejects_redirect_status() {
        let url = WikipediaRequests::new("enwiki")
            .unwrap()
            .current_revision(42)
            .unwrap();
        let (mut client, path) =
            fake_curl("printf '{\"query\":{}}'\nprintf 'FW_HTTP_CODE:200\\n' >&2");
        assert_eq!(client.get(&url, 1024).unwrap(), br#"{"query":{}}"#);
        fs::remove_file(path).unwrap();

        let (mut client, path) =
            fake_curl("printf 'redirect body'\nprintf 'FW_HTTP_CODE:302\\n' >&2");
        assert_eq!(
            client.get(&url, 1024),
            Err(CurlTransportError::HttpStatus(302))
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn caps_streamed_body_without_trusting_content_length() {
        let url = WikipediaRequests::new("enwiki")
            .unwrap()
            .rendered_revision(333)
            .unwrap();
        let (mut client, path) = fake_curl("printf '123456789'\nprintf 'FW_HTTP_CODE:200\\n' >&2");
        assert_eq!(
            client.get(&url, 8),
            Err(CurlTransportError::ResponseTooLarge)
        );
        fs::remove_file(path).unwrap();
    }
}
