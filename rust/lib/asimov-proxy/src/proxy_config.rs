// This is free and unencumbered software released into the public domain.

//! Upstream connection establishment, optionally through a proxy.
//!
//! Environment discovery consults conventional environment variables,
//! consulted in this order: `https_proxy`, `HTTPS_PROXY`, `all_proxy`,
//! `ALL_PROXY`. (Since the upstream endpoint is always HTTPS, `http_proxy`
//! does not apply.) The `no_proxy`/`NO_PROXY` exclusion list is honored.
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
    /// Connect directly to the target.
    #[default]
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
    /// Determines the proxy configuration for `target_host` from the
    /// conventional environment variables for an HTTPS upstream.
    ///
    /// Reads the first nonempty value from `https_proxy`, `HTTPS_PROXY`,
    /// `all_proxy`, and `ALL_PROXY`, unless `no_proxy` or `NO_PROXY` excludes
    /// the host. Exclusions support `*`, exact hosts, and domain suffixes.
    #[cfg(feature = "std")]
    pub fn from_env(target_host: &str) -> Result<Self, Error> {
        if no_proxy_matches(target_host) {
            return Ok(Self::Direct);
        }
        match env_var(&["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"]) {
            Some(input) => Self::parse(&input),
            None => Ok(Self::Direct),
        }
    }

    /// Parses a proxy URL such as `http://user:pass@host:port` or
    /// `socks5h://host:port`.
    pub fn parse(input: &str) -> Result<Self, Error> {
        // A bare `host:port` is conventionally an HTTP proxy:
        let url = if input.contains("://") {
            url::Url::parse(input)
        } else {
            url::Url::parse(&format!("http://{}", input))
        }?;

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

/// Returns the first nonempty environment variable among `names`.
#[cfg(feature = "std")]
fn env_var(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

/// Checks whether `host` is excluded from proxying by `no_proxy`/`NO_PROXY`.
#[cfg(feature = "std")]
fn no_proxy_matches(host: &str) -> bool {
    let Some(no_proxy) = env_var(&["no_proxy", "NO_PROXY"]) else {
        return false;
    };
    no_proxy_list_matches(&no_proxy, host)
}

#[cfg(any(feature = "std", test))]
fn no_proxy_list_matches(no_proxy: &str, host: &str) -> bool {
    no_proxy.split(',').any(|entry| {
        let entry = entry.trim().trim_start_matches('.');
        !entry.is_empty()
            && (entry == "*"
                || host.eq_ignore_ascii_case(entry)
                || host
                    .len()
                    .checked_sub(entry.len())
                    .and_then(|index| host.split_at_checked(index))
                    .is_some_and(|(prefix, suffix)| {
                        prefix.ends_with('.') && suffix.eq_ignore_ascii_case(entry)
                    }))
    })
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
                ProxyConfig::Direct => panic!("expected proxy credentials"),
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
                ProxyConfig::Direct => panic!("expected a proxy"),
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
    fn no_proxy_matching() {
        assert!(no_proxy_list_matches("*", "openrouter.ai"));
        assert!(no_proxy_list_matches("openrouter.ai", "openrouter.ai"));
        assert!(no_proxy_list_matches(".openrouter.ai", "api.openrouter.ai"));
        assert!(no_proxy_list_matches(
            "example.com, openrouter.ai",
            "openrouter.ai"
        ));
        assert!(!no_proxy_list_matches("example.com", "openrouter.ai"));
        assert!(!no_proxy_list_matches("router.ai", "openrouter.ai"));
        assert!(!no_proxy_list_matches("a.ai", "é.ai"));
    }
}
