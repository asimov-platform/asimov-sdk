// This is free and unencumbered software released into the public domain.

//! Shared HTTP proxy options and endpoint validation (`proxy` feature).
//!
//! `system-proxy` adds `OriginProxy` for construction-time environment/native
//! discovery. `reqwest` adds its `configure_reqwest` adapter. A resolved policy
//! belongs to one HTTP(S) origin; it must not configure a multi-origin client.
//! `hyper` adds `proxy_for_hyper`, including the effective bypass decision.
//! Native bypass grammars remain separate from `NO_PROXY` syntax.

use alloc::{format, string::String};
use core::fmt;
use url::Url;

#[cfg(feature = "system-proxy")]
mod system;
#[cfg(feature = "system-proxy")]
pub use system::OriginProxy;
#[cfg(feature = "hyper")]
mod hyper;
#[cfg(feature = "reqwest")]
mod reqwest;
#[cfg(all(test, feature = "reqwest"))]
mod test_support;

/// How an HTTP client selects its proxy endpoint.
///
/// With `system-proxy`, resolve once with `OriginProxy::resolve` before building
/// an origin-scoped client. This type itself needs only alloc.
#[derive(Clone, Default, Eq, PartialEq)]
pub enum ProxyMode {
    /// Read environment and native static settings. Per-scheme environment
    /// endpoints override native settings, followed by ALL_PROXY. This is the
    /// default. See `OriginProxy` for discovery and bypass semantics.
    #[default]
    Automatic,
    /// Connect directly, without reading proxy environment or native settings.
    /// Ignores [`ProxyOptions::no_proxy`].
    Direct,
    /// Use this endpoint instead of environment or native proxy endpoints.
    ///
    /// Accepts `http://`, `https://`, `socks4://`, `socks4a://`, `socks5://`, and
    /// `socks5h://` URLs, or a bare `host[:port]` for HTTP. Ports default to 80,
    /// 443, or 1080 respectively. IPv6 addresses require brackets. URL userinfo
    /// supports existing HTTP Basic/SOCKS5 authentication; percent-encode
    /// reserved characters. Debug output omits the URL, including credentials.
    ///
    /// Invalid endpoints return [`Error::InvalidProxyUrl`] at construction,
    /// even when a bypass rule matches. Paths other than `/`, queries, fragments,
    /// whitespace, and port zero are rejected. SOCKS hostnames must be ASCII or
    /// punycode. A failed connection never falls back to another proxy or direct
    /// access. This explicit choice also applies when `REQUEST_METHOD` is set.
    Explicit(String),
}

impl fmt::Debug for ProxyMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Automatic => f.write_str("Automatic"),
            Self::Direct => f.write_str("Direct"),
            Self::Explicit(_) => f.write_str("Explicit([REDACTED])"),
        }
    }
}

/// Client-local proxy selection and bypass policy.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProxyOptions {
    /// Endpoint selection. Defaults to [`ProxyMode::Automatic`].
    pub mode: ProxyMode,
    /// Bypass list in `NO_PROXY` syntax: comma-separated domains/subdomains,
    /// IP literals/CIDRs, or `*` for every host, including IP literals.
    /// Matching does not resolve DNS names to test CIDRs. Port-qualified rules
    /// and native-only syntax such as `<local>` are not supported here.
    ///
    /// - `None` inherits `NO_PROXY`/`no_proxy`. In automatic mode, an empty
    ///   environment list permits native exclusions when a native proxy wins.
    ///   Explicit mode never reads native exclusions.
    /// - `Some(list)` replaces both environment and native exclusions.
    /// - `Some(String::new())` disables all bypass rules.
    /// - `Some("*".into())` selects direct access for every host.
    ///
    /// Direct mode ignores this field. Automatic mode retains CGI protection
    /// regardless of the override. Configuration never changes the environment.
    pub no_proxy: Option<String>,
}

#[cfg(feature = "system-proxy")]
fn bypasses_all(list: &str) -> bool {
    list.split(',').any(|rule| rule.trim() == "*")
}

/// Proxy configuration failures, separate from HTTP integrity failures.
/// Diagnostics name settings or fixed reasons, never endpoint contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    /// Native settings could not be read/decoded, or an enabled endpoint is invalid.
    #[error("invalid native proxy setting: {0}")]
    InvalidProxy(&'static str),
    /// Invalid explicit endpoint, including unsupported schemes.
    #[error("invalid explicit proxy URL: {0}")]
    InvalidProxyUrl(&'static str),
    /// The native source requires an unsupported resolver.
    #[error("unsupported automatic proxy source: {0}; configure an explicit proxy")]
    UnsupportedProxy(&'static str),
    /// A target is not HTTP(S), or differs from the resolved origin.
    #[error("invalid proxy target origin: {0}")]
    InvalidOrigin(&'static str),
}

/// Validates an explicit HTTP(S)/SOCKS proxy URL or bare `host[:port]`.
///
/// The returned URL retains userinfo; do not log it without redaction. SOCKS
/// default ports are supplied by transport adapters. No environment or native
/// settings are read. See [`ProxyMode::Explicit`] for accepted endpoint syntax.
///
/// # Errors
///
/// Returns [`Error::InvalidProxyUrl`] with a credential-free reason.
pub fn parse_endpoint(input: &str) -> Result<Url, Error> {
    if input.is_empty() || input.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Error::InvalidProxyUrl(
            "expected a nonempty URL without whitespace",
        ));
    }
    let url = environment_proxy(input).ok_or(Error::InvalidProxyUrl(
        "expected an HTTP(S) or SOCKS4/4a/5/5h endpoint with a host",
    ))?;
    if url.port() == Some(0) {
        return Err(Error::InvalidProxyUrl("port must be nonzero"));
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err(Error::InvalidProxyUrl(
            "paths, queries, and fragments are not supported",
        ));
    }
    // Non-special URL schemes escape Unicode hosts instead of using IDNA.
    // Require a usable ASCII/punycode name rather than an escaped DNS name.
    if !url
        .host_str()
        .is_some_and(|host| host.is_ascii() && !host.contains('%'))
    {
        return Err(Error::InvalidProxyUrl("host must be ASCII or punycode"));
    }
    Ok(url)
}

fn environment_proxy(input: &str) -> Option<Url> {
    if input.is_empty() {
        return None;
    }
    let url = if input.contains("://") {
        Url::parse(input)
    } else {
        Url::parse(&format!("http://{input}"))
    }
    .ok()?;
    (url.host_str().is_some()
        && matches!(
            url.scheme(),
            "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
        ))
    .then_some(url)
}

#[cfg(test)]
mod tests;
