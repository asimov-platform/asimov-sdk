// This is free and unencumbered software released into the public domain.

//! Construction-time routing for the executor's immutable service origin.
//!
//! Native bypass grammars stay separate from NO_PROXY. Ordinary reqwest proxies
//! preserve authentication, tunneling, and TLS; custom callbacks cannot report
//! errors and do not honor attached NoProxy rules in reqwest 0.13.

use super::Error;
use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use core::fmt;
use reqwest::{ClientBuilder, NoProxy, Proxy, Url};

#[cfg(any(windows, target_os = "linux", test))]
mod native;
#[cfg(windows)]
mod windows;

/// How an HTTP executor selects its proxy endpoint.
///
/// Configure with [`super::Executor::new_with_options`]. Selection happens once,
/// before constructing the shared connection pool and identity-discovery client.
#[derive(Clone, Default, Eq, PartialEq)]
pub enum ProxyMode {
    /// Read environment and native static settings using the documented
    /// [precedence](super#proxies-and-destination-lookup). This is the default.
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

/// Executor-local proxy selection and bypass policy.
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

#[derive(Default)]
pub(super) struct Environment {
    pub proxy: String,
    pub all_proxy: String,
    pub no_proxy: String,
    pub no_proxy_override: bool,
    pub cgi: bool,
}

impl Environment {
    pub(super) fn capture(url: &Url) -> Self {
        fn first(upper: &str, lower: &str) -> String {
            // An empty uppercase value still takes precedence over lowercase.
            std::env::var(upper)
                .or_else(|_| std::env::var(lower))
                .unwrap_or_default()
        }
        Self {
            proxy: match url.scheme() {
                "https" => first("HTTPS_PROXY", "https_proxy"),
                _ => first("HTTP_PROXY", "http_proxy"),
            },
            all_proxy: first("ALL_PROXY", "all_proxy"),
            no_proxy: first("NO_PROXY", "no_proxy"),
            no_proxy_override: false,
            cgi: std::env::var_os("REQUEST_METHOD").is_some(),
        }
    }

    pub(super) fn use_native_bypass(&self) -> bool {
        !self.no_proxy_override && self.no_proxy.is_empty()
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Route {
    /// No native endpoint; use per-scheme environment settings, then ALL_PROXY.
    Automatic,
    Direct,
    Native(Url),
}

/// Prepared before TLS/client construction.
pub(super) struct Configuration {
    proxy: Option<Url>,
    no_proxy: String,
}

pub(super) fn resolve(url: &Url, options: &ProxyOptions) -> Result<Configuration, Error> {
    resolve_options(
        options,
        || Environment::capture(url),
        |env| {
            #[cfg(target_os = "macos")]
            return super::macos_proxy::read(url, env);
            #[cfg(windows)]
            return windows::read(url, env);
            #[cfg(target_os = "linux")]
            return native::read_linux(url, env);
            #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
            {
                let _ = env;
                Ok(Route::Automatic)
            }
        },
    )
}

fn resolve_options(
    options: &ProxyOptions,
    capture: impl FnOnce() -> Environment,
    native: impl FnOnce(&Environment) -> Result<Route, Error>,
) -> Result<Configuration, Error> {
    match &options.mode {
        ProxyMode::Direct => Ok(Configuration {
            proxy: None,
            no_proxy: String::new(),
        }),
        ProxyMode::Explicit(input) => {
            // Validate before bypass selection or discovery, including NO_PROXY=*.
            let proxy = explicit_proxy(input)?;
            let no_proxy = options
                .no_proxy
                .clone()
                .unwrap_or_else(|| capture().no_proxy);
            Ok(Configuration {
                proxy: Some(proxy),
                no_proxy,
            })
        },
        ProxyMode::Automatic => {
            let mut env = capture();
            if let Some(no_proxy) = &options.no_proxy {
                env.no_proxy.clone_from(no_proxy);
                env.no_proxy_override = true;
            }
            resolve_environment(&env, || native(&env))
        },
    }
}

pub(super) fn select(
    env: &Environment,
    native: impl FnOnce() -> Result<Route, Error>,
) -> Result<Route, Error> {
    if env.cgi || env.no_proxy.split(',').any(|rule| rule.trim() == "*") {
        // Reqwest's wildcard only matches DNS names, not IP-literal URLs.
        Ok(Route::Direct)
    } else if !env.proxy.is_empty() {
        // Even an unusable per-scheme override suppresses native discovery;
        // preserve the previous fallback to ALL_PROXY in that case.
        Ok(Route::Automatic)
    } else {
        native()
    }
}

fn resolve_environment(
    env: &Environment,
    native: impl FnOnce() -> Result<Route, Error>,
) -> Result<Configuration, Error> {
    let proxy = match select(env, native)? {
        Route::Automatic => {
            environment_proxy(&env.proxy).or_else(|| environment_proxy(&env.all_proxy))
        },
        Route::Direct => None,
        Route::Native(proxy) => Some(proxy),
    };
    Ok(Configuration {
        proxy,
        no_proxy: env.no_proxy.clone(),
    })
}

impl Configuration {
    pub(super) fn apply(self, builder: ClientBuilder) -> Result<ClientBuilder, Error> {
        // Never let reqwest rediscover settings, including when a dependency
        // enables its system-proxy feature through feature unification.
        let builder = builder.no_proxy();
        if self.no_proxy.split(',').any(|rule| rule.trim() == "*") {
            return Ok(builder);
        }
        Ok(match self.proxy {
            Some(proxy) => {
                let builder = if proxy.scheme() == "socks4" {
                    builder.dns_resolver(Arc::new(Socks4Resolver))
                } else {
                    builder
                };
                builder.proxy(Proxy::all(proxy)?.no_proxy(NoProxy::from_string(&self.no_proxy)))
            },
            None => builder,
        })
    }
}

#[cfg(test)]
pub(super) fn configure_with(
    builder: ClientBuilder,
    env: &Environment,
    native: impl FnOnce() -> Result<Route, Error>,
) -> Result<ClientBuilder, Error> {
    resolve_environment(env, native)?.apply(builder)
}

fn explicit_proxy(input: &str) -> Result<Url, Error> {
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

/// Reqwest selects only the first locally resolved SOCKS destination. SOCKS4
/// cannot encode IPv6, so prefer IPv4 while retaining IPv6 for the proxy itself.
/// Use the system resolver, without changing DNS policy or caching results.
struct Socks4Resolver;

impl reqwest::dns::Resolve for Socks4Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            use std::net::ToSocketAddrs;
            let mut addresses = tokio::task::spawn_blocking(move || {
                (name.as_str(), 0)
                    .to_socket_addrs()
                    .map(|addresses| addresses.collect::<Vec<_>>())
            })
            .await??;
            addresses.sort_by_key(|address| address.is_ipv6());
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
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
