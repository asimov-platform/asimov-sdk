// This is free and unencumbered software released into the public domain.

//! Construction-time routing for the executor's immutable service origin.
//!
//! Native bypass grammars stay separate from NO_PROXY. Ordinary reqwest proxies
//! preserve authentication, tunneling, and TLS; custom callbacks cannot report
//! errors and do not honor attached NoProxy rules in reqwest 0.13.

use super::Error;
use alloc::{boxed::Box, format, string::String, sync::Arc, vec::Vec};
use reqwest::{ClientBuilder, NoProxy, Proxy, Url};

#[cfg(any(windows, target_os = "linux", test))]
mod native;
#[cfg(windows)]
mod windows;

#[derive(Default)]
pub(super) struct Environment {
    pub proxy: String,
    pub all_proxy: String,
    pub no_proxy: String,
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
            cgi: std::env::var_os("REQUEST_METHOD").is_some(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Route {
    /// No native endpoint; use per-scheme environment settings, then ALL_PROXY.
    Automatic,
    Direct,
    Native(Url),
}

pub(super) fn configure(builder: ClientBuilder, url: &Url) -> Result<ClientBuilder, Error> {
    let env = Environment::capture(url);
    configure_with(builder, &env, || {
        #[cfg(target_os = "macos")]
        return super::macos_proxy::read(url, &env);
        #[cfg(windows)]
        return windows::read(url, &env);
        #[cfg(target_os = "linux")]
        return native::read_linux(url, &env);
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        Ok(Route::Automatic)
    })
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

pub(super) fn configure_with(
    builder: ClientBuilder,
    env: &Environment,
    native: impl FnOnce() -> Result<Route, Error>,
) -> Result<ClientBuilder, Error> {
    let proxy = match select(env, native)? {
        Route::Automatic => {
            environment_proxy(&env.proxy).or_else(|| environment_proxy(&env.all_proxy))
        },
        Route::Direct => None,
        Route::Native(proxy) => Some(proxy),
    };
    // Never let reqwest rediscover native settings, including when another
    // dependency enables its system-proxy feature through feature unification.
    let builder = builder.no_proxy();
    Ok(match proxy {
        Some(proxy) => {
            let builder = if proxy.scheme() == "socks4" {
                builder.dns_resolver(Arc::new(Socks4Resolver))
            } else {
                builder
            };
            builder.proxy(Proxy::all(proxy)?.no_proxy(NoProxy::from_string(&env.no_proxy)))
        },
        None => builder,
    })
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
