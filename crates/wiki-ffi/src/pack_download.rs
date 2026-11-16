//! MOBPACK-001 official pack downloads over HTTPS (ADR 0009).
//!
//! A deliberately small HTTP/1.1 GET client on rustls with the operating
//! system's certificate verifier; certificate checks are never disabled.
//! It talks only to the official release host and, for at most **one**
//! redirect from it, to GitHub's release-asset hosts. Responses must carry
//! an exact `Content-Length` (no chunked or compressed bodies) and are
//! bounded by the size the signed catalog declares. Interrupted downloads
//! resume with `Range` requests into a partial file.
//!
//! Nothing downloaded here is trusted: catalogs and packs are verified
//! against pinned publisher keys and every byte afterwards.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};

/// The only host an official download may start from.
pub const RELEASE_HOST: &str = "github.com";
/// The only path prefix an official download may start from.
pub const RELEASE_PATH_PREFIX: &str = "/ekkus93/foundation-wikipedia/releases/download/";
/// Hosts GitHub redirects release assets to; reachable only by redirect.
pub const ASSET_HOSTS: &[&str] = &[
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];
/// The signed catalog's stable location.
pub const CATALOG_URL: &str =
    "https://github.com/ekkus93/foundation-wikipedia/releases/download/catalog/catalog.json";
pub const CATALOG_SIGNATURE_URL: &str =
    "https://github.com/ekkus93/foundation-wikipedia/releases/download/catalog/catalog.json.sig";

const MAX_URL_BYTES: usize = 8192;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
/// Short socket timeout so cancellation is noticed promptly.
const READ_POLL: Duration = Duration::from_millis(250);
/// A response that makes no progress for this long has stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadError {
    /// Not an `https://` URL this client accepts.
    InvalidUrl,
    /// A URL outside the official release host, or a redirect elsewhere.
    NotAllowed,
    /// Certificates cannot be verified on this platform yet.
    Unsupported,
    /// Connection, TLS or protocol failure (including bad certificates).
    Transport,
    Timeout,
    Cancelled,
    /// An unexpected HTTP status.
    Http(u16),
    /// The response is larger than allowed or its size is wrong.
    SizeMismatch,
    /// Writing the partial file failed.
    Storage,
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DownloadError {}

/// A parsed `https://host/path` URL (no port, credentials or fragment).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsUrl {
    pub host: String,
    /// Path and query, starting with `/`.
    pub path: String,
}

impl HttpsUrl {
    pub fn parse(url: &str) -> Result<Self, DownloadError> {
        if url.len() > MAX_URL_BYTES
            || !url
                .bytes()
                .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\\' | b'<' | b'>' | b'`'))
        {
            return Err(DownloadError::InvalidUrl);
        }
        let rest = url
            .strip_prefix("https://")
            .ok_or(DownloadError::InvalidUrl)?;
        let (host, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let host_ok = !host.is_empty()
            && host.len() <= 253
            && host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-');
        if !host_ok || path.contains('#') {
            return Err(DownloadError::InvalidUrl);
        }
        Ok(Self {
            host: host.to_owned(),
            path: path.to_owned(),
        })
    }

    fn is_release(&self) -> bool {
        self.host == RELEASE_HOST
            && self.path.starts_with(RELEASE_PATH_PREFIX)
            && !self.path.contains("/../")
            && !self.path.contains("/./")
    }

    fn is_asset_host(&self) -> bool {
        ASSET_HOSTS.contains(&self.host.as_str())
    }
}

/// Something that can be read and written, i.e. a TLS stream.
pub trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

/// Opens a verified TLS connection to `host` on port 443.
pub trait Connector: Send + Sync {
    fn connect(
        &self,
        host: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Box<dyn Stream>, DownloadError>;
}

fn ring() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// TCP + rustls with an explicit client configuration. Production uses the
/// OS verifier; tests use a local CA and point every host at a local port.
pub struct TlsConnector {
    tls: Arc<ClientConfig>,
    /// Test-only override: connect here instead of `host:443`.
    address: Option<std::net::SocketAddr>,
}

impl TlsConnector {
    /// The production connector: OS certificate verification, port 443.
    pub fn platform() -> Result<Self, DownloadError> {
        if !wiki_ai::hosted_http::platform_verifier_ready() {
            return Err(DownloadError::Unsupported);
        }
        use rustls_platform_verifier::BuilderVerifierExt;
        let tls = ClientConfig::builder_with_provider(ring())
            .with_safe_default_protocol_versions()
            .map_err(|_| DownloadError::Unsupported)?
            .with_platform_verifier()
            .map_err(|_| DownloadError::Unsupported)?
            .with_no_client_auth();
        Ok(Self {
            tls: Arc::new(tls),
            address: None,
        })
    }

    /// Test-only: trust `tls`'s roots and connect every host to `address`.
    #[cfg(test)]
    pub fn for_test(tls: ClientConfig, address: std::net::SocketAddr) -> Self {
        Self {
            tls: Arc::new(tls),
            address: Some(address),
        }
    }
}

fn is_wait(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
    )
}

impl Connector for TlsConnector {
    fn connect(
        &self,
        host: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Box<dyn Stream>, DownloadError> {
        let addresses: Vec<_> = match self.address {
            Some(address) => vec![address],
            None => (host, 443)
                .to_socket_addrs()
                .map_err(|_| DownloadError::Transport)?
                .collect(),
        };
        let mut socket = None;
        for address in addresses {
            if cancelled() {
                return Err(DownloadError::Cancelled);
            }
            if let Ok(connected) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                socket = Some(connected);
                break;
            }
        }
        let socket = socket.ok_or(DownloadError::Transport)?;
        socket
            .set_read_timeout(Some(READ_POLL))
            .and_then(|()| socket.set_write_timeout(Some(CONNECT_TIMEOUT)))
            .map_err(|_| DownloadError::Transport)?;
        let name = ServerName::try_from(host.to_owned()).map_err(|_| DownloadError::InvalidUrl)?;
        let connection =
            ClientConnection::new(self.tls.clone(), name).map_err(|_| DownloadError::Transport)?;
        let mut tls = StreamOwned::new(connection, socket);
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        while tls.conn.is_handshaking() {
            if cancelled() {
                return Err(DownloadError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(DownloadError::Timeout);
            }
            match tls.conn.complete_io(&mut tls.sock) {
                Ok(_) => {}
                Err(error) if is_wait(&error) => {}
                // Includes certificate verification failures.
                Err(_) => return Err(DownloadError::Transport),
            }
        }
        Ok(Box::new(tls))
    }
}

/// Reads through short socket timeouts, checking cancellation, until data,
/// end of stream, or a stall.
struct PatientReader<'a> {
    inner: Box<dyn Stream>,
    cancelled: &'a dyn Fn() -> bool,
}

impl Read for PatientReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let stall = Instant::now() + STALL_TIMEOUT;
        loop {
            if (self.cancelled)() {
                return Err(std::io::Error::other(Interrupt::Cancelled));
            }
            match self.inner.read(buf) {
                Ok(n) => return Ok(n),
                Err(error) if is_wait(&error) => {
                    if Instant::now() >= stall {
                        return Err(std::io::Error::other(Interrupt::Stalled));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[derive(Debug)]
enum Interrupt {
    Cancelled,
    Stalled,
}

impl std::fmt::Display for Interrupt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Interrupt {}

fn map_read(error: std::io::Error) -> DownloadError {
    match error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Interrupt>())
    {
        Some(Interrupt::Cancelled) => DownloadError::Cancelled,
        Some(Interrupt::Stalled) => DownloadError::Timeout,
        None => DownloadError::Transport,
    }
}

struct Response<'a> {
    status: u16,
    location: Option<String>,
    content_length: Option<u64>,
    content_range: Option<String>,
    body: BufReader<PatientReader<'a>>,
}

fn request<'a>(
    connector: &dyn Connector,
    url: &HttpsUrl,
    range_from: Option<u64>,
    cancelled: &'a dyn Fn() -> bool,
) -> Result<Response<'a>, DownloadError> {
    let mut stream = connector.connect(&url.host, cancelled)?;
    let range = range_from
        .map(|from| format!("Range: bytes={from}-\r\n"))
        .unwrap_or_default();
    let head = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: foundation-wikipedia-reader\r\n\
         Accept: */*\r\nAccept-Encoding: identity\r\n{range}Connection: close\r\n\r\n",
        url.path, url.host
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|_| DownloadError::Transport)?;
    let mut body = BufReader::with_capacity(
        CHUNK,
        PatientReader {
            inner: stream,
            cancelled,
        },
    );
    let mut header_bytes = 0;
    let mut lines = Vec::new();
    loop {
        let mut line = Vec::new();
        let n = (&mut body)
            .take((MAX_HEADER_BYTES - header_bytes) as u64)
            .read_until(b'\n', &mut line)
            .map_err(map_read)?;
        header_bytes += n;
        if n == 0 || !line.ends_with(b"\r\n") || header_bytes >= MAX_HEADER_BYTES {
            return Err(DownloadError::Transport);
        }
        line.truncate(line.len() - 2);
        if line.is_empty() {
            break;
        }
        lines.push(String::from_utf8(line).map_err(|_| DownloadError::Transport)?);
    }
    let status_line = lines.first().ok_or(DownloadError::Transport)?;
    let mut parts = status_line.splitn(3, ' ');
    let (Some("HTTP/1.1" | "HTTP/1.0"), Some(code)) = (parts.next(), parts.next()) else {
        return Err(DownloadError::Transport);
    };
    let status: u16 = code.parse().map_err(|_| DownloadError::Transport)?;
    let mut response = Response {
        status,
        location: None,
        content_length: None,
        content_range: None,
        body,
    };
    for line in &lines[1..] {
        let (name, value) = line.split_once(':').ok_or(DownloadError::Transport)?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "location" => response.location = Some(value.to_owned()),
            "content-length" => {
                if response.content_length.is_some() {
                    return Err(DownloadError::Transport);
                }
                response.content_length =
                    Some(value.parse().map_err(|_| DownloadError::Transport)?);
            }
            "content-range" => response.content_range = Some(value.to_owned()),
            "transfer-encoding" => return Err(DownloadError::Transport),
            "content-encoding" if !value.eq_ignore_ascii_case("identity") => {
                return Err(DownloadError::Transport)
            }
            _ => {}
        }
    }
    Ok(response)
}

/// Follow at most one redirect: from the release host to an asset host.
fn open<'a>(
    connector: &dyn Connector,
    url: &str,
    range_from: Option<u64>,
    cancelled: &'a dyn Fn() -> bool,
) -> Result<Response<'a>, DownloadError> {
    let first = HttpsUrl::parse(url)?;
    if !first.is_release() {
        return Err(DownloadError::NotAllowed);
    }
    let response = request(connector, &first, range_from, cancelled)?;
    if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        return Ok(response);
    }
    let location = response.location.ok_or(DownloadError::Transport)?;
    let target = HttpsUrl::parse(&location).map_err(|_| DownloadError::NotAllowed)?;
    if !target.is_asset_host() {
        return Err(DownloadError::NotAllowed);
    }
    let second = request(connector, &target, range_from, cancelled)?;
    if matches!(second.status, 301 | 302 | 303 | 307 | 308) {
        return Err(DownloadError::NotAllowed);
    }
    Ok(second)
}

/// Fetch a small file (catalog or signature) of at most `max_bytes`.
pub fn fetch_small(
    connector: &dyn Connector,
    url: &str,
    max_bytes: usize,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, DownloadError> {
    let mut response = open(connector, url, None, cancelled)?;
    if response.status != 200 {
        return Err(DownloadError::Http(response.status));
    }
    let length = response.content_length.ok_or(DownloadError::Transport)?;
    if length > max_bytes as u64 {
        return Err(DownloadError::SizeMismatch);
    }
    let mut bytes = Vec::with_capacity(length as usize);
    (&mut response.body)
        .take(length)
        .read_to_end(&mut bytes)
        .map_err(map_read)?;
    if bytes.len() as u64 != length {
        return Err(DownloadError::Transport);
    }
    Ok(bytes)
}

/// Download `url` into `partial`, resuming from its current length, until
/// it holds exactly `expected_bytes`. A partial file longer than expected
/// is discarded. On a network failure the partial file is kept for a later
/// resume; `progress(done, total)` is called as bytes arrive.
pub fn download_resumable(
    connector: &dyn Connector,
    url: &str,
    expected_bytes: u64,
    partial: &Path,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(u64, u64),
) -> Result<(), DownloadError> {
    if expected_bytes == 0 {
        return Err(DownloadError::SizeMismatch);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(partial)
        .map_err(|_| DownloadError::Storage)?;
    let mut have = file.metadata().map_err(|_| DownloadError::Storage)?.len();
    if have > expected_bytes {
        file.set_len(0).map_err(|_| DownloadError::Storage)?;
        have = 0;
    }
    progress(have, expected_bytes);
    if have == expected_bytes {
        return Ok(());
    }
    let mut response = open(connector, url, (have > 0).then_some(have), cancelled)?;
    let length = response.content_length.ok_or(DownloadError::Transport)?;
    match response.status {
        206 if have > 0 => {
            let expected_range = format!("bytes {have}-{}/{expected_bytes}", expected_bytes - 1);
            if response.content_range.as_deref() != Some(expected_range.as_str())
                || length != expected_bytes - have
            {
                return Err(DownloadError::SizeMismatch);
            }
        }
        200 => {
            // The server ignored the range: start over.
            if length != expected_bytes {
                return Err(DownloadError::SizeMismatch);
            }
            file.set_len(0).map_err(|_| DownloadError::Storage)?;
            have = 0;
        }
        status => return Err(DownloadError::Http(status)),
    }
    file.seek(SeekFrom::Start(have))
        .map_err(|_| DownloadError::Storage)?;
    let mut buf = vec![0u8; CHUNK];
    while have < expected_bytes {
        let want = buf.len().min((expected_bytes - have) as usize);
        let n = response.body.read(&mut buf[..want]).map_err(map_read)?;
        if n == 0 {
            // Keep what arrived for a later resume.
            let _ = file.sync_data();
            return Err(DownloadError::Transport);
        }
        file.write_all(&buf[..n])
            .map_err(|_| DownloadError::Storage)?;
        have += n as u64;
        progress(have, expected_bytes);
    }
    file.sync_all().map_err(|_| DownloadError::Storage)?;
    Ok(())
}

/// Open a finished download for installation.
pub fn open_downloaded(partial: &Path) -> Result<File, DownloadError> {
    File::open(partial).map_err(|_| DownloadError::Storage)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{RootCertStore, ServerConfig, ServerConnection};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::thread;

    /// What the fake server saw: (SNI host, request head).
    pub(crate) type Seen = Arc<Mutex<Vec<(String, String)>>>;

    /// A local TLS server whose certificate covers the release and asset
    /// hosts (and an "evil" host). `respond` maps (sni, request head) to raw
    /// response bytes for each connection.
    pub(crate) struct FakeGithub {
        pub connector: TlsConnector,
        pub seen: Seen,
    }

    pub(crate) fn fake_github(
        respond: impl Fn(&str, &str) -> Vec<u8> + Send + Sync + 'static,
    ) -> FakeGithub {
        let names = vec![
            RELEASE_HOST.to_owned(),
            ASSET_HOSTS[0].to_owned(),
            ASSET_HOSTS[1].to_owned(),
            "evil.example".to_owned(),
        ];
        let ca_key = rcgen::KeyPair::generate().unwrap();
        let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = rcgen::Issuer::new(ca_params, ca_key);
        let leaf_key = rcgen::KeyPair::generate().unwrap();
        let leaf = rcgen::CertificateParams::new(names)
            .unwrap()
            .signed_by(&leaf_key, &issuer)
            .unwrap();
        let server_config = Arc::new(
            ServerConfig::builder_with_provider(ring())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(leaf.der().to_vec())],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der())),
                )
                .unwrap(),
        );
        let mut roots = RootCertStore::empty();
        roots.add(CertificateDer::from(ca.der().to_vec())).unwrap();
        let client = ClientConfig::builder_with_provider(ring())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let respond = Arc::new(respond);
        thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(socket) = socket else { return };
                let server_config = server_config.clone();
                let log = log.clone();
                let respond = respond.clone();
                thread::spawn(move || {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let connection = ServerConnection::new(server_config).unwrap();
                    let mut tls = StreamOwned::new(connection, socket);
                    let mut bytes = Vec::new();
                    let mut buf = [0u8; 4096];
                    while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                        match tls.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => bytes.extend_from_slice(&buf[..n]),
                        }
                    }
                    let sni = tls.conn.server_name().unwrap_or_default().to_owned();
                    let head = String::from_utf8_lossy(&bytes).into_owned();
                    log.lock().unwrap().push((sni.clone(), head.clone()));
                    let _ = tls.write_all(&respond(&sni, &head));
                    tls.conn.send_close_notify();
                    let _ = tls.flush();
                });
            }
        });
        FakeGithub {
            connector: TlsConnector::for_test(client, address),
            seen,
        }
    }

    pub(crate) fn ok(body: &[u8]) -> Vec<u8> {
        let mut out =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend_from_slice(body);
        out
    }

    pub(crate) fn redirect(to: &str) -> Vec<u8> {
        format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n").into_bytes()
    }

    /// Serves `body` honouring `Range: bytes=N-`.
    pub(crate) fn ranged(body: &[u8], head: &str) -> Vec<u8> {
        let from = head
            .lines()
            .find_map(|line| line.strip_prefix("Range: bytes="))
            .and_then(|rest| rest.strip_suffix('-'))
            .and_then(|n| n.parse::<usize>().ok());
        match from {
            Some(from) => {
                let mut out = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\n\r\n",
                    body.len() - 1,
                    body.len(),
                    body.len() - from
                )
                .into_bytes();
                out.extend_from_slice(&body[from..]);
                out
            }
            None => ok(body),
        }
    }

    pub(crate) const ASSET_URL: &str = "https://objects.githubusercontent.com/asset/1?sig=abc";

    fn release(name: &str) -> String {
        format!("https://{RELEASE_HOST}{RELEASE_PATH_PREFIX}packs-20261101/{name}")
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pack-download-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const NEVER: &dyn Fn() -> bool = &|| false;

    #[test]
    fn urls_are_restricted_to_https_release_paths() {
        assert!(HttpsUrl::parse(&release("a.wpack")).unwrap().is_release());
        for bad in [
            "http://github.com/ekkus93/foundation-wikipedia/releases/download/x",
            "https://github.com:8443/ekkus93/foundation-wikipedia/releases/download/x",
            "https://user@github.com/ekkus93/foundation-wikipedia/releases/download/x",
            "https://GitHub.com/x",
            "https://github.com/a b",
        ] {
            assert!(
                HttpsUrl::parse(bad).map_or(true, |url| !url.is_release()),
                "{bad}"
            );
        }
        let other_repo = HttpsUrl::parse("https://github.com/someone/else/releases/download/x");
        assert!(!other_repo.unwrap().is_release());
        let traversal = HttpsUrl::parse(&release("../../../../other/x")).unwrap();
        assert!(!traversal.is_release());
    }

    #[test]
    fn follows_one_redirect_to_an_asset_host_and_fetches_exact_bytes() {
        let server = fake_github(|sni, _| match sni {
            RELEASE_HOST => redirect(ASSET_URL),
            _ => ok(b"catalog bytes"),
        });
        let bytes = fetch_small(&server.connector, &release("catalog.json"), 1024, NEVER).unwrap();
        assert_eq!(bytes, b"catalog bytes");
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].0, ASSET_HOSTS[0]);
        assert!(seen[1].1.starts_with("GET /asset/1?sig=abc HTTP/1.1\r\n"));
        assert!(seen[1].1.contains("Accept-Encoding: identity\r\n"));
    }

    #[test]
    fn other_redirects_compression_chunking_and_oversize_are_refused() {
        let cases: Vec<(Vec<u8>, DownloadError)> = vec![
            (
                redirect("https://evil.example/x"),
                DownloadError::NotAllowed,
            ),
            (
                redirect("http://objects.githubusercontent.com/x"),
                DownloadError::NotAllowed,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 1\r\n\r\nx".to_vec(),
                DownloadError::Transport,
            ),
            (
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n0\r\n\r\n"
                    .to_vec(),
                DownloadError::Transport,
            ),
            (ok(&[b'x'; 2000]), DownloadError::SizeMismatch),
            (
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                DownloadError::Http(404),
            ),
        ];
        for (response, expected) in cases {
            let server = fake_github(move |_, _| response.clone());
            assert_eq!(
                fetch_small(&server.connector, &release("x"), 1024, NEVER),
                Err(expected)
            );
        }
        // A second redirect, even between allowed hosts, is refused.
        let server = fake_github(|_, _| redirect(ASSET_URL));
        assert_eq!(
            fetch_small(&server.connector, &release("x"), 1024, NEVER),
            Err(DownloadError::NotAllowed)
        );
        // Nothing outside the release path is ever requested.
        let server = fake_github(|_, _| ok(b"x"));
        assert_eq!(
            fetch_small(&server.connector, "https://evil.example/x", 1024, NEVER),
            Err(DownloadError::NotAllowed)
        );
        assert!(server.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn an_untrusted_certificate_is_a_transport_error() {
        let server = fake_github(|_, _| ok(b"x"));
        let stranger = fake_github(|_, _| ok(b"x"));
        // Trust one test CA but connect to the other server.
        let connector = TlsConnector {
            tls: server.connector.tls.clone(),
            address: stranger.connector.address,
        };
        assert_eq!(
            fetch_small(&connector, &release("x"), 1024, NEVER),
            Err(DownloadError::Transport)
        );
    }

    #[test]
    fn an_interrupted_download_resumes_with_a_range_request() {
        let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let full = body.clone();
        let server = fake_github(move |sni, head| match sni {
            RELEASE_HOST => redirect(ASSET_URL),
            _ => ranged(&full, head),
        });
        let dir = temp("resume");
        let partial = dir.join("pack.part");
        std::fs::write(&partial, &body[..70_000]).unwrap();
        let last = Mutex::new((0, 0));
        download_resumable(
            &server.connector,
            &release("pack.wpack"),
            body.len() as u64,
            &partial,
            NEVER,
            &|done, total| *last.lock().unwrap() = (done, total),
        )
        .unwrap();
        assert_eq!(std::fs::read(&partial).unwrap(), body);
        assert_eq!(
            *last.lock().unwrap(),
            (body.len() as u64, body.len() as u64)
        );
        let seen = server.seen.lock().unwrap();
        assert!(seen
            .iter()
            .all(|(_, head)| head.contains("Range: bytes=70000-\r\n")));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_server_ignoring_the_range_restarts_and_a_wrong_range_is_refused() {
        let body = b"0123456789".to_vec();
        let full = body.clone();
        let server = fake_github(move |_, _| ok(&full));
        let dir = temp("restart");
        let partial = dir.join("pack.part");
        std::fs::write(&partial, b"XXXX").unwrap();
        download_resumable(
            &server.connector,
            &release("p"),
            10,
            &partial,
            NEVER,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(std::fs::read(&partial).unwrap(), body);

        let bad = fake_github(|_, _| {
            b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9/10\r\nContent-Length: 10\r\n\r\n0123456789".to_vec()
        });
        std::fs::write(&partial, b"0123").unwrap();
        assert_eq!(
            download_resumable(
                &bad.connector,
                &release("p"),
                10,
                &partial,
                NEVER,
                &|_, _| {}
            ),
            Err(DownloadError::SizeMismatch)
        );
        // A size other than the catalog's is refused.
        let wrong = fake_github(|_, _| ok(b"01234567890"));
        std::fs::remove_file(&partial).unwrap();
        assert_eq!(
            download_resumable(
                &wrong.connector,
                &release("p"),
                10,
                &partial,
                NEVER,
                &|_, _| {}
            ),
            Err(DownloadError::SizeMismatch)
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_truncated_response_keeps_the_partial_file_and_cancellation_stops() {
        let server =
            fake_github(|_, _| b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n01234".to_vec());
        let dir = temp("truncated");
        let partial = dir.join("pack.part");
        assert_eq!(
            download_resumable(
                &server.connector,
                &release("p"),
                10,
                &partial,
                NEVER,
                &|_, _| {}
            ),
            Err(DownloadError::Transport)
        );
        assert_eq!(std::fs::read(&partial).unwrap(), b"01234");

        let cancelled: &dyn Fn() -> bool = &|| true;
        assert_eq!(
            download_resumable(
                &server.connector,
                &release("p"),
                10,
                &partial,
                cancelled,
                &|_, _| {}
            ),
            Err(DownloadError::Cancelled)
        );
        std::fs::remove_dir_all(dir).ok();
    }
}

/// Live check against the published release (ADR 0009); run explicitly with
/// `cargo test -p wiki-ffi -- --ignored live_`.
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore = "needs the network and the published catalog release"]
    fn live_catalog_and_pack_signature_download() {
        let connector = TlsConnector::platform().unwrap();
        let never: &dyn Fn() -> bool = &|| false;
        let catalog = fetch_small(&connector, CATALOG_URL, 1 << 20, never).unwrap();
        assert!(catalog.starts_with(b"{"));
        let signature = fetch_small(&connector, CATALOG_SIGNATURE_URL, 512, never).unwrap();
        assert!(signature.starts_with(b"foundation-wikipedia/catalog-signature/v1\n"));
    }
}
