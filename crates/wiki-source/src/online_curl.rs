//! Desktop/host-only, bounded HTTPS adapter for Wikimedia Action API reads.
//!
//! Uses the system curl executable with certificate verification enabled,
//! no redirect following, fixed deadlines, and a streaming response cap.
//! Android needs its own platform HTTP transport. A successful response
//! authenticates the TLS peer, NOT offline media completeness or pack trust.

use std::io::Read;
use std::process::{Command, Stdio};

use url::Url;

use crate::online_fetch::MediaWikiTransport;
use crate::online_requests::WikipediaRequests;

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

/// A failed exec with ETXTBSY starts no child and is safe to retry. Keep
/// retries bounded; permission errors and missing executables fail immediately.
fn retry_executable_busy<T>(mut spawn: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut retries = 0;
    loop {
        match spawn() {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy && retries < 3 => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20 * retries));
            }
            result => return result,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CurlTransportError {
    InvalidRequest,
    InvalidLimit,
    SpawnFailed,
    IoFailure,
    ResponseTooLarge,
    DiagnosticTooLarge,
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

/// curl's write-out marker must be unique and canonical. A truncated or
/// ambiguous diagnostic stream cannot authenticate an HTTP 200 response.
fn parse_http_status(diagnostics: &[u8]) -> Result<u16, CurlTransportError> {
    let mut status = None;
    for line in diagnostics.split(|byte| *byte == b'\n') {
        if let Some(raw) = line.strip_prefix(b"FW_HTTP_CODE:") {
            if status.is_some() || raw.len() != 3 || !raw.iter().all(u8::is_ascii_digit) {
                return Err(CurlTransportError::ProcessFailed);
            }
            status = Some(
                u16::from(raw[0] - b'0') * 100
                    + u16::from(raw[1] - b'0') * 10
                    + u16::from(raw[2] - b'0'),
            );
        }
    }
    status.ok_or(CurlTransportError::ProcessFailed)
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
        let mut child = retry_executable_busy(|| {
            Command::new(&self.executable)
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
                    "%{stderr}\\nFW_HTTP_CODE:%{http_code}\\n",
                    "--",
                    url,
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        })
        .map_err(|error| {
            // Keep the public error stable, but retain a safe OS-level
            // diagnostic for intermittent runner spawn failures.
            eprintln!(
                "wiki-source curl spawn failed: kind={:?}, raw_os_error={:?}",
                error.kind(),
                error.raw_os_error()
            );
            CurlTransportError::SpawnFailed
        })?;

        // Drain both subprocess pipes concurrently without deadlocking. An
        // oversized diagnostic stream is rejected rather than retaining its
        // tail: otherwise an earlier conflicting HTTP marker could disappear.
        let mut stderr = child.stderr.take().ok_or(CurlTransportError::IoFailure)?;
        let stderr_reader = std::thread::spawn(move || -> std::io::Result<(Vec<u8>, bool)> {
            let mut diagnostics = Vec::new();
            let mut oversized = false;
            let mut buffer = [0u8; 8192];
            loop {
                let count = stderr.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                if !oversized {
                    if count > MAX_DIAGNOSTIC_BYTES.saturating_sub(diagnostics.len()) {
                        oversized = true;
                    } else {
                        diagnostics.extend_from_slice(&buffer[..count]);
                    }
                }
            }
            Ok((diagnostics, oversized))
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
        let (stderr_bytes, oversized) = stderr_reader
            .join()
            .map_err(|_| CurlTransportError::IoFailure)?
            .map_err(|_| CurlTransportError::IoFailure)?;
        if oversized {
            return Err(CurlTransportError::DiagnosticTooLarge);
        }
        let http_status = parse_http_status(&stderr_bytes)?;
        if http_status != 200 && http_status != 0 {
            return Err(CurlTransportError::HttpStatus(http_status));
        }
        if !status.success() || http_status != 200 {
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
    fn rejects_oversized_diagnostics_even_with_final_http_status() {
        let url = WikipediaRequests::new("enwiki")
            .unwrap()
            .current_revision(42)
            .unwrap();
        // Drain more than an OS pipe without deadlock, but do not trust the
        // tail: an earlier status marker could have been discarded.
        let noise = r#"i=0
while [ "$i" -lt 3000 ]; do
    printf 'verbose diagnostic filler 0123456789abcdefghijklmnopqrstuvwxyz\n' >&2
    i=$((i + 1))
done
"#;
        for code in ["200", "503"] {
            let (mut client, path) = fake_curl(&format!(
                "{noise}\nprintf 'FW_HTTP_CODE:{code}\\n' >&2\nprintf 'ok'"
            ));
            assert_eq!(
                client.get(&url, 128),
                Err(CurlTransportError::DiagnosticTooLarge),
                "{code}"
            );
            fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn accepts_bounded_diagnostics_but_refuses_ambiguous_http_markers() {
        let url = WikipediaRequests::new("enwiki")
            .unwrap()
            .current_revision(42)
            .unwrap();
        let noise = r#"i=0
while [ "$i" -lt 100 ]; do
    printf 'ordinary curl diagnostics\n' >&2
    i=$((i + 1))
done
"#;
        let (mut client, path) = fake_curl(&format!(
            "{noise}\nprintf 'FW_HTTP_CODE:200\\n' >&2\nprintf 'ok'"
        ));
        assert_eq!(client.get(&url, 128).unwrap(), b"ok");
        fs::remove_file(path).unwrap();

        // Only one exact, three-ASCII-digit status marker can authorize a
        // response. Even a later valid 200 cannot override an earlier marker.
        for code in ["+200", "0200", "200x", "2O0", "200 ", ""] {
            let (mut client, path) =
                fake_curl(&format!("printf 'ok'\nprintf 'FW_HTTP_CODE:{code}\\n' >&2"));
            assert_eq!(
                client.get(&url, 128),
                Err(CurlTransportError::ProcessFailed),
                "{code:?}"
            );
            fs::remove_file(path).unwrap();
        }
        for earlier in ["200", "503"] {
            let (mut client, path) = fake_curl(&format!(
                "printf 'ok'\nprintf 'FW_HTTP_CODE:{earlier}\\nFW_HTTP_CODE:200\\n' >&2"
            ));
            assert_eq!(
                client.get(&url, 128),
                Err(CurlTransportError::ProcessFailed),
                "{earlier}"
            );
            fs::remove_file(path).unwrap();
        }
        let (mut client, path) = fake_curl("printf 'ok'");
        assert_eq!(
            client.get(&url, 128),
            Err(CurlTransportError::ProcessFailed)
        );
        fs::remove_file(path).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn retries_executable_busy_only_and_never_retries_permission_denied() {
        let mut attempts = 0;
        let result = retry_executable_busy(|| {
            attempts += 1;
            if attempts <= 2 {
                Err(std::io::Error::from_raw_os_error(26))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);

        let mut exhausted = 0;
        let result: std::io::Result<()> = retry_executable_busy(|| {
            exhausted += 1;
            Err(std::io::Error::from_raw_os_error(26))
        });
        assert_eq!(result.unwrap_err().raw_os_error(), Some(26));
        assert_eq!(exhausted, 4);

        let mut denied = 0;
        let result: std::io::Result<()> = retry_executable_busy(|| {
            denied += 1;
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(denied, 1);
    }

    #[test]
    fn reports_nonexecutable_curl_as_spawn_failure() {
        let url = WikipediaRequests::new("enwiki")
            .unwrap()
            .current_revision(42)
            .unwrap();
        // A directory cannot be executed. This exercises the spawn-error
        // boundary without depending on process-resource exhaustion in CI.
        let mut client = CurlTransport {
            executable: "/".into(),
        };
        assert_eq!(client.get(&url, 128), Err(CurlTransportError::SpawnFailed));
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
