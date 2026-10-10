//! Fail-closed endpoint classification for opt-in local LLM connections.
//!
//! Do not infer "LAN" from an arbitrary DNS name: it may resolve to a
//! public service or change addresses between validation and connection.
//! The transport must additionally pin the validated destination address
//! when it is implemented. This module does not open network connections.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::Locality;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointError {
    InvalidUrl,
    UnsupportedScheme,
    InvalidPort,
    UnapprovedHost,
    UnsupportedPath,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalEndpoint {
    base_url: String,
    locality: Locality,
}

impl LocalEndpoint {
    /// Accept explicit loopback or private-network IP literals only, plus
    /// the exact hostname "localhost". No credentials, query, fragment,
    /// DNS-based LAN host, or public IP can be silently classified as local.
    pub fn parse(input: &str) -> Result<Self, EndpointError> {
        if input.is_empty()
            || input.trim() != input
            || !input.is_ascii()
            || input.bytes().any(|b| b.is_ascii_control())
            || input.contains(['@', '?', '#', '\\', '%'])
        {
            return Err(EndpointError::InvalidUrl);
        }
        let (scheme, rest) = input.split_once("://").ok_or(EndpointError::InvalidUrl)?;
        if !matches!(scheme, "http" | "https") {
            return Err(EndpointError::UnsupportedScheme);
        }
        let (authority, path) = match rest.split_once('/') {
            Some((host, suffix)) => (host, format!("/{suffix}")),
            None => (rest, String::new()),
        };
        if authority.is_empty() {
            return Err(EndpointError::InvalidUrl);
        }
        if !matches!(path.as_str(), "" | "/" | "/v1" | "/v1/") {
            return Err(EndpointError::UnsupportedPath);
        }
        let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
            let (address, tail) = bracketed.split_once(']').ok_or(EndpointError::InvalidUrl)?;
            let port = if tail.is_empty() {
                None
            } else {
                Some(tail.strip_prefix(':').ok_or(EndpointError::InvalidUrl)?)
            };
            (address, port)
        } else {
            match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        if let Some(port) = port {
            if port.is_empty()
                || !port.bytes().all(|b| b.is_ascii_digit())
                || port.parse::<u16>().ok().filter(|p| *p != 0).is_none()
            {
                return Err(EndpointError::InvalidPort);
            }
        }
        let locality = if host.eq_ignore_ascii_case("localhost") {
            Locality::Localhost
        } else {
            let address = host
                .parse::<IpAddr>()
                .map_err(|_| EndpointError::UnapprovedHost)?;
            match address {
                IpAddr::V4(ip) if ip.is_loopback() => Locality::Localhost,
                IpAddr::V6(ip) if ip == Ipv6Addr::LOCALHOST => Locality::Localhost,
                IpAddr::V4(ip) if private_ipv4(ip) => Locality::LocalNetwork,
                IpAddr::V6(ip) if unique_local_ipv6(ip) => Locality::LocalNetwork,
                _ => return Err(EndpointError::UnapprovedHost),
            }
        };
        // Brackets are mandatory for IPv6 URLs, even without a port.
        if host.contains(':') && !authority.starts_with('[') {
            return Err(EndpointError::InvalidUrl);
        }
        Ok(Self {
            base_url: input.to_owned(),
            locality,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn locality(&self) -> Locality {
        self.locality
    }
}

fn private_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

fn unique_local_ipv6(ip: Ipv6Addr) -> bool {
    ip.segments()[0] & 0xfe00 == 0xfc00
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_localhost_and_private_lan_without_cloud_fallback() {
        for url in [
            "http://localhost:11434",
            "http://127.0.0.1:8080/v1",
            "http://127.2.3.4",
            "http://[::1]:11434/v1",
        ] {
            let endpoint = LocalEndpoint::parse(url).unwrap();
            assert_eq!(endpoint.locality(), Locality::Localhost, "{url}");
            assert_eq!(endpoint.base_url(), url);
        }
        for url in [
            "http://10.1.2.3:11434",
            "http://172.31.1.1/v1",
            "https://192.168.0.50:443/v1/",
            "http://[fd00::42]:11434",
        ] {
            assert_eq!(
                LocalEndpoint::parse(url).unwrap().locality(),
                Locality::LocalNetwork,
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_public_dns_metadata_and_unspecified_addresses() {
        for url in [
            "https://api.openai.com/v1",
            "http://example.org",
            "http://ollama.lan:11434",
            "http://8.8.8.8",
            "http://169.254.169.254",
            "http://0.0.0.0",
            "http://172.32.0.1",
            "http://[2001:4860:4860::8888]",
            "http://[::ffff:127.0.0.1]",
        ] {
            assert_eq!(
                LocalEndpoint::parse(url),
                Err(EndpointError::UnapprovedHost),
                "{url}"
            );
        }
    }

    #[test]
    fn rejects_credentials_unsafe_paths_and_malformed_authorities() {
        for url in [
            "http://user:password@localhost:11434",
            "http://localhost:11434/v1/../../admin",
            "http://localhost:11434/api",
            "http://localhost:11434/v1?token=secret",
            "http://localhost:11434/#fragment",
            "http://localhost:11434/%2e%2e",
            "http://[::1",
            "http://[::1]junk",
            "http://127.0.0.1:11434:443",
            "http://localhost:0",
            "http://localhost:65536",
            "http://localhost:",
            "http://localhost:abc",
            "http://localhost:11434\\v1",
            "http://localhost:11434/v1\n",
            "http://localhost:11434/v1 ",
        ] {
            assert!(LocalEndpoint::parse(url).is_err(), "{url}");
        }
        assert_eq!(
            LocalEndpoint::parse("ftp://localhost"),
            Err(EndpointError::UnsupportedScheme)
        );
    }
}
