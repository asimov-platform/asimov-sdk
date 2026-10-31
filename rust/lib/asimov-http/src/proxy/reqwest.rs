// This is free and unencumbered software released into the public domain.

//! Reqwest-specific installation of a resolved single-origin policy.

use super::{OriginProxy, system::Configuration};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::{fmt, time::Duration};
use reqwest::{ClientBuilder, NoProxy, Proxy, dns::Resolve};

/// The connect limit of [`OriginProxy::client_builder`].
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The per-read limit of [`OriginProxy::client_builder`].
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Client limits and DNS for [`OriginProxy::client_builder_with`] (`reqwest`).
///
/// The default matches [`OriginProxy::client_builder`]: 10 seconds to connect,
/// 30 seconds per read, and system DNS.
#[derive(Clone)]
pub struct ClientOptions {
    /// The connect limit, including proxy handshakes. `None` sets no limit.
    pub connect_timeout: Option<Duration>,
    /// The limit on each read, including the wait for response headers.
    /// `None` sets no limit, for streams whose callers bound idle time.
    pub read_timeout: Option<Duration>,
    /// The resolver for locally resolved hostnames, or `None` for system DNS.
    /// Its results for SOCKS4 destinations are reordered to prefer IPv4.
    pub dns_resolver: Option<Arc<dyn Resolve>>,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Some(CONNECT_TIMEOUT),
            read_timeout: Some(READ_TIMEOUT),
            dns_resolver: None,
        }
    }
}

impl fmt::Debug for ClientOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientOptions")
            .field("connect_timeout", &self.connect_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("dns_resolver", &self.dns_resolver.as_ref().map(|_| ".."))
            .finish()
    }
}

impl OriginProxy {
    /// Installs this origin's proxy policy on a reqwest builder (`reqwest`).
    ///
    /// Replaces existing proxies, disables automatic proxy discovery and
    /// redirects, and prefers system-resolved IPv4 for SOCKS4. TLS, service
    /// credentials, and timeouts remain the caller's settings. Apply this after
    /// any other proxy/redirect settings and then build the client.
    ///
    /// **The resulting raw reqwest client must only send requests to
    /// [`Self::origin`].** Use [`Self::check_origin`] before dispatching dynamic
    /// URLs. Requests to other origins and manual redirects need separately
    /// resolved clients; this adapter does not enforce request URL restrictions.
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), Box<dyn core::error::Error>> {
    /// use asimov_http::proxy::{OriginProxy, ProxyOptions};
    /// let url = reqwest::Url::parse("https://example.com/resource")?;
    /// let proxy = OriginProxy::resolve(&url, &ProxyOptions::default())?;
    /// let client = proxy.configure_reqwest(reqwest::Client::builder())?.build()?;
    /// proxy.check_origin(&url)?;
    /// let response = client.get(url).send().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns reqwest's error if the selected endpoint cannot configure a proxy.
    pub fn configure_reqwest(
        &self,
        builder: ClientBuilder,
    ) -> Result<ClientBuilder, reqwest::Error> {
        apply(builder, &self.configuration, None)
    }

    /// Starts a reqwest client builder for this origin with standard limits.
    ///
    /// Connecting may take 10 seconds and each read, including the wait for
    /// response headers, 30 seconds; there is no total limit. This policy is
    /// installed via [`Self::configure_reqwest`], whose origin restriction
    /// applies. Callers may add a user agent, decoding preferences, or a total
    /// `timeout`, but must not change proxy or redirect settings. Use
    /// [`Self::client_builder_with`] for other limits or a DNS resolver.
    ///
    /// # Errors
    ///
    /// Returns reqwest's error if the selected endpoint cannot configure a proxy.
    pub fn client_builder(&self) -> Result<ClientBuilder, reqwest::Error> {
        self.client_builder_with(&ClientOptions::default())
    }

    /// Starts a reqwest client builder for this origin with `options`.
    ///
    /// As [`Self::client_builder`], with the given limits and DNS resolver.
    /// Callers must not replace the resolver afterward: SOCKS4 destinations
    /// are resolved locally through a wrapper that prefers IPv4 addresses.
    ///
    /// # Errors
    ///
    /// Returns reqwest's error if the selected endpoint cannot configure a proxy.
    pub fn client_builder_with(
        &self,
        options: &ClientOptions,
    ) -> Result<ClientBuilder, reqwest::Error> {
        let mut builder = ClientBuilder::new();
        if let Some(connect) = options.connect_timeout {
            builder = builder.connect_timeout(connect);
        }
        if let Some(read) = options.read_timeout {
            builder = builder.read_timeout(read);
        }
        apply(builder, &self.configuration, options.dns_resolver.clone())
    }
}

pub(super) fn apply(
    builder: ClientBuilder,
    configuration: &Configuration,
    resolver: Option<Arc<dyn Resolve>>,
) -> Result<ClientBuilder, reqwest::Error> {
    // Reqwest must not rediscover settings through unified system-proxy features.
    let builder = builder
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none());
    let proxy = configuration
        .proxy
        .as_ref()
        .filter(|_| !super::bypasses_all(&configuration.no_proxy));
    let builder = match (resolver, proxy) {
        (resolver, Some(proxy)) if proxy.scheme() == "socks4" => {
            builder.dns_resolver(Socks4Resolver(resolver))
        },
        (Some(resolver), _) => builder.dns_resolver(resolver),
        (None, _) => builder,
    };
    Ok(match proxy {
        Some(proxy) => builder.proxy(
            Proxy::all(proxy.clone())?.no_proxy(NoProxy::from_string(&configuration.no_proxy)),
        ),
        None => builder,
    })
}

/// Reqwest selects only the first locally resolved SOCKS destination. Prefer
/// IPv4 for SOCKS4, while retaining IPv6 for the proxy itself. Wraps the
/// caller's resolver, if any, and otherwise keeps system DNS.
struct Socks4Resolver(Option<Arc<dyn Resolve>>);

impl Resolve for Socks4Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let inner = self.0.clone();
        Box::pin(async move {
            let mut addresses = match inner {
                Some(inner) => inner.resolve(name).await?.collect::<Vec<_>>(),
                None => {
                    use std::net::ToSocketAddrs;
                    tokio::task::spawn_blocking(move || {
                        (name.as_str(), 0)
                            .to_socket_addrs()
                            .map(|addresses| addresses.collect::<Vec<_>>())
                    })
                    .await??
                },
            };
            addresses.sort_by_key(|address| address.is_ipv6());
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests;
