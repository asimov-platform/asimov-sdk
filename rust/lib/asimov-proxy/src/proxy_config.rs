// This is free and unencumbered software released into the public domain.

//! Upstream connection establishment, optionally through a proxy.
//!
//! Discovery, endpoint validation, and bypass decisions use `asimov-http`.
//! Uppercase environment values, including empty values, precede lowercase
//! values. Per-scheme endpoints precede native settings, then ALL_PROXY.
//!
//! Supported proxy URL schemes:
//!
//! - `http://` — HTTP proxy (tunneled with `CONNECT`)
//! - `https://` — HTTPS proxy (TLS to the proxy itself, then `CONNECT`)
//! - `socks5://` — SOCKS5 proxy (target DNS resolved locally)
//! - `socks5h://` — SOCKS5 proxy (target DNS resolved by the proxy)

use crate::Error;
use alloc::{
    format,
    string::{String, ToString},
};
use base64::Engine as _;

/// How to reach the upstream server.
#[derive(Clone, Default)]
pub enum ProxyConfig {
    /// Resolve shared environment/native policy for the destination at setup.
    #[default]
    Automatic,

    /// Connect directly to the target.
    Direct,

    /// Tunnel through an HTTP(S) proxy using `CONNECT`.
    HttpConnect {
        /// Proxy hostname or unbracketed IP address.
        host: String,
        /// Proxy TCP port.
        port: u16,
        /// Whether to speak TLS to the proxy itself (an `https://` proxy).
        tls: bool,
        /// Pre-encoded `Proxy-Authorization: Basic` credentials.
        basic_auth: Option<String>,
    },

    /// Tunnel through a SOCKS5 proxy.
    Socks5 {
        /// Proxy hostname or unbracketed IP address.
        host: String,
        /// Proxy TCP port.
        port: u16,
        /// Optional username and password.
        auth: Option<(String, String)>,
        /// Whether to resolve target DNS through the proxy (`socks5h://`).
        remote_dns: bool,
    },
}

impl core::fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Automatic => f.write_str("Automatic"),
            Self::Direct => f.write_str("Direct"),
            Self::HttpConnect {
                host,
                port,
                tls,
                basic_auth,
            } => f
                .debug_struct("HttpConnect")
                .field("host", host)
                .field("port", port)
                .field("tls", tls)
                .field("basic_auth", &basic_auth.as_ref().map(|_| "[REDACTED]"))
                .finish(),
            Self::Socks5 {
                host,
                port,
                auth,
                remote_dns,
            } => f
                .debug_struct("Socks5")
                .field("host", host)
                .field("port", port)
                .field("auth", &auth.as_ref().map(|_| "[REDACTED]"))
                .field("remote_dns", remote_dns)
                .finish(),
        }
    }
}

impl ProxyConfig {
    #[cfg(feature = "std")]
    pub(crate) fn for_target(self, target: &url::Url) -> Result<Self, Error> {
        match self {
            Self::Automatic => Self::from_options(target, &crate::UpstreamProxyOptions::default()),
            config => Ok(config),
        }
    }

    /// Resolves shared environment/native proxy policy for an HTTPS host.
    ///
    /// Uppercase variables take precedence, including empty values. Honors
    /// native exclusions, NO_PROXY domains/IPs/CIDRs/`*`, and CGI protection.
    /// The returned route is scoped to this host on port 443. For a URL with
    /// a different scheme/port or local overrides, use [`Self::from_options`].
    #[cfg(feature = "std")]
    pub fn from_env(target_host: &str) -> Result<Self, Error> {
        let host = match target_host.parse::<core::net::Ipv6Addr>() {
            Ok(address) => format!("[{address}]"),
            Err(_) => target_host.into(),
        };
        let mut url = url::Url::parse("https://localhost/").expect("valid HTTPS base URL");
        url.set_host(Some(&host))?;
        Self::from_options(&url, &crate::UpstreamProxyOptions::default())
    }

    /// Resolves a route for one origin using shared proxy selection and bypass
    /// options. Use the returned configuration only for this origin.
    ///
    /// Discovery/validation errors propagate. A selected protocol unsupported
    /// by this connector (currently SOCKS4/4a) returns an explicit error.
    #[cfg(feature = "std")]
    pub fn from_options(
        target: &url::Url,
        options: &crate::UpstreamProxyOptions,
    ) -> Result<Self, Error> {
        let policy = asimov_http::proxy::OriginProxy::resolve(target, options)?;
        match policy.proxy_for_hyper()? {
            Some(endpoint) => Self::from_endpoint(endpoint),
            None => Ok(Self::Direct),
        }
    }

    /// Parses a proxy URL such as `http://user:pass@host:port` or
    /// `socks5h://host:port`.
    ///
    /// Produces an explicit route with no inherited bypass rules. URL syntax
    /// uses shared validation; paths, queries, fragments, and port zero fail.
    /// Use [`Self::Automatic`] or, with `std`, `from_options` for bypass policy.
    pub fn parse(input: &str) -> Result<Self, Error> {
        Self::from_endpoint(&asimov_http::proxy::parse_endpoint(input)?)
    }

    fn from_endpoint(url: &url::Url) -> Result<Self, Error> {
        // Socket address tuples and TLS server names need an unbracketed IP.
        let host = match url.host().ok_or(Error::MissingProxyHost)? {
            url::Host::Domain(host) => host.to_string(),
            url::Host::Ipv4(ip) => ip.to_string(),
            url::Host::Ipv6(ip) => ip.to_string(),
        };

        let username = percent_encoding::percent_decode_str(url.username())
            .decode_utf8()
            .map_err(|_| Error::InvalidProxyUsername)?;
        let password = url
            .password()
            .map(|password| percent_encoding::percent_decode_str(password).decode_utf8())
            .transpose()
            .map_err(|_| Error::InvalidProxyPassword)?;

        match url.scheme() {
            "http" | "https" => {
                let tls = url.scheme() == "https";
                let port = url.port().unwrap_or(if tls { 443 } else { 80 });
                let basic_auth = if !username.is_empty() || password.is_some() {
                    let credentials =
                        format!("{}:{}", username, password.as_deref().unwrap_or_default());
                    Some(base64::engine::general_purpose::STANDARD.encode(credentials))
                } else {
                    None
                };
                Ok(Self::HttpConnect {
                    host,
                    port,
                    tls,
                    basic_auth,
                })
            },

            "socks5" | "socks5h" => {
                let port = url.port().unwrap_or(1080);
                let auth = if !username.is_empty() || password.is_some() {
                    Some((
                        username.to_string(),
                        password.as_deref().unwrap_or_default().to_string(),
                    ))
                } else {
                    None
                };
                Ok(Self::Socks5 {
                    host,
                    port,
                    auth,
                    remote_dns: url.scheme() == "socks5h",
                })
            },

            scheme => Err(Error::UnsupportedProxyScheme(scheme.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_credentials_are_percent_decoded() {
        for scheme in ["http", "https", "socks5", "socks5h"] {
            let config = ProxyConfig::parse(&format!(
                "{scheme}://user%40domain:p%3Aa%25ss+@proxy.example"
            ))
            .unwrap();
            match config {
                ProxyConfig::HttpConnect { basic_auth, .. } => {
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(basic_auth.unwrap())
                        .unwrap();
                    assert_eq!(decoded, b"user@domain:p:a%ss+");
                },
                ProxyConfig::Socks5 { auth, .. } => {
                    assert_eq!(auth, Some(("user@domain".into(), "p:a%ss+".into())));
                },
                ProxyConfig::Automatic | ProxyConfig::Direct => {
                    panic!("expected proxy credentials")
                },
            }
            let error = ProxyConfig::parse(&format!("{scheme}://user:secret%FF@proxy.example"))
                .unwrap_err()
                .to_string();
            assert!(error.contains("UTF-8"));
            assert!(!error.contains("secret"));
        }
    }

    #[cfg(feature = "std")]
    #[test]
    fn ipv6_proxy_hosts_resolve_without_url_brackets() {
        use alloc::vec::Vec;
        use std::net::ToSocketAddrs;

        for input in [
            "http://[::1]:8080",
            "https://[::1]:8080",
            "socks5://[::1]:8080",
            "socks5h://[::1]:8080",
            "[::1]:8080",
        ] {
            let config = ProxyConfig::parse(input).unwrap();
            let (host, port) = match config {
                ProxyConfig::HttpConnect { host, port, .. }
                | ProxyConfig::Socks5 { host, port, .. } => (host, port),
                ProxyConfig::Automatic | ProxyConfig::Direct => panic!("expected a proxy"),
            };
            assert_eq!(host, "::1", "{input}");
            let addresses: Vec<_> = (host.as_str(), port).to_socket_addrs().unwrap().collect();
            assert_eq!(
                addresses,
                ["[::1]:8080".parse::<core::net::SocketAddr>().unwrap()]
            );
        }
    }

    #[test]
    fn diagnostics_redact_proxy_credentials() {
        let encoded = base64::engine::general_purpose::STANDARD.encode("private-user:private-pass");
        for scheme in ["http", "https", "socks5", "socks5h"] {
            let config = ProxyConfig::parse(&format!(
                "{scheme}://private-user:private-pass@proxy.example:8080"
            ))
            .unwrap();
            let debug = format!("{config:?}");
            assert!(debug.contains("proxy.example"));
            assert!(debug.contains("REDACTED"));
            for secret in ["private-user", "private-pass", &encoded] {
                assert!(!debug.contains(secret));
            }
            let error = ProxyConfig::parse(&format!(
                "{scheme}://private-user:private-pass@proxy.example:invalid"
            ))
            .unwrap_err()
            .to_string();
            assert!(!error.contains("private-user"));
            assert!(!error.contains("private-pass"));
        }
    }

    #[test]
    fn parse_http_proxy() {
        let config = ProxyConfig::parse("http://proxy.example:3128").unwrap();
        let ProxyConfig::HttpConnect {
            host,
            port,
            tls,
            basic_auth,
        } = config
        else {
            panic!("expected HttpConnect, got: {:?}", config)
        };
        assert_eq!(host, "proxy.example");
        assert_eq!(port, 3128);
        assert!(!tls);
        assert!(basic_auth.is_none());
    }

    #[test]
    fn parse_bare_host_port_as_http_proxy() {
        let config = ProxyConfig::parse("proxy.example:8080").unwrap();
        assert!(matches!(
            config,
            ProxyConfig::HttpConnect {
                tls: false,
                port: 8080,
                ..
            }
        ));
    }

    #[test]
    fn parse_https_proxy_with_auth() {
        let config = ProxyConfig::parse("https://user:pass@proxy.example").unwrap();
        let ProxyConfig::HttpConnect {
            host,
            port,
            tls,
            basic_auth,
        } = config
        else {
            panic!("expected HttpConnect, got: {:?}", config)
        };
        assert_eq!(host, "proxy.example");
        assert_eq!(port, 443);
        assert!(tls);
        assert_eq!(basic_auth.as_deref(), Some("dXNlcjpwYXNz")); // "user:pass"
    }

    #[test]
    fn parse_socks5_proxy() {
        let config = ProxyConfig::parse("socks5://127.0.0.1").unwrap();
        assert!(matches!(
            config,
            ProxyConfig::Socks5 {
                port: 1080,
                remote_dns: false,
                auth: None,
                ..
            }
        ));

        let config = ProxyConfig::parse("socks5h://user:pass@127.0.0.1:9050").unwrap();
        let ProxyConfig::Socks5 {
            port,
            remote_dns,
            auth,
            ..
        } = config
        else {
            panic!("expected Socks5, got: {:?}", config)
        };
        assert_eq!(port, 9050);
        assert!(remote_dns);
        assert_eq!(auth, Some(("user".to_string(), "pass".to_string())));
    }

    #[test]
    fn parse_unsupported_scheme() {
        assert!(ProxyConfig::parse("ftp://proxy.example").is_err());
    }

    #[test]
    fn explicit_endpoints_use_shared_validation() {
        for endpoint in [
            "http://proxy.example/path",
            "http://proxy.example?query",
            "http://proxy.example#fragment",
            "http://proxy.example:0",
            " proxy.example",
        ] {
            assert!(matches!(ProxyConfig::parse(endpoint), Err(Error::Proxy(_))));
        }
    }
}
