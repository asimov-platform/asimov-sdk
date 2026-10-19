// This is free and unencumbered software released into the public domain.

//! Reqwest-specific installation of a resolved single-origin policy.

use super::{OriginProxy, system::Configuration};
use alloc::{boxed::Box, sync::Arc, vec::Vec};
use reqwest::{ClientBuilder, NoProxy, Proxy};

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
        apply(builder, &self.configuration)
    }
}

pub(super) fn apply(
    builder: ClientBuilder,
    configuration: &Configuration,
) -> Result<ClientBuilder, reqwest::Error> {
    // Reqwest must not rediscover settings through unified system-proxy features.
    let builder = builder
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none());
    if super::bypasses_all(&configuration.no_proxy) {
        return Ok(builder);
    }
    Ok(match &configuration.proxy {
        Some(proxy) => {
            let builder = if proxy.scheme() == "socks4" {
                builder.dns_resolver(Arc::new(Socks4Resolver))
            } else {
                builder
            };
            builder.proxy(
                Proxy::all(proxy.clone())?.no_proxy(NoProxy::from_string(&configuration.no_proxy)),
            )
        },
        None => builder,
    })
}

/// Reqwest selects only the first locally resolved SOCKS destination. Prefer
/// IPv4 for SOCKS4, while retaining IPv6 for the proxy itself. Keep system DNS.
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

#[cfg(test)]
mod tests;
