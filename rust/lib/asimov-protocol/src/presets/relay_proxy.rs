// This is free and unencumbered software released into the public domain.

//! Selection of Iroh's endpoint-wide proxy for relay connections.

use asimov_http::{
    proxy::{OriginProxy, ProxyOptions},
    reqwest::Url,
};

/// Selects the proxy for connections to `relays`, or `None` to connect directly.
///
/// Iroh accepts a single HTTP(S) proxy for all of an endpoint's HTTP traffic,
/// so a proxy is selected only when every relay resolves to the same HTTP(S)
/// endpoint after bypass rules. Relays that disagree, SOCKS endpoints (which
/// Iroh's relay client cannot use), and discovery failures fall back to direct
/// connections. Warnings never include proxy URLs, which may hold credentials.
pub(super) fn select(relays: impl IntoIterator<Item = Url>, options: &ProxyOptions) -> Option<Url> {
    let mut selected: Option<Option<Url>> = None;
    for relay in relays {
        let proxy = match OriginProxy::resolve(&relay, options)
            .and_then(|policy| policy.proxy_for_hyper().map(Option::<&Url>::cloned))
        {
            Ok(proxy) => proxy,
            Err(_error) => {
                #[cfg(feature = "tracing")]
                tracing::warn!(error = %_error, "relay proxy discovery failed; connecting directly");
                return None;
            },
        };
        match &selected {
            None => selected = Some(proxy),
            Some(previous) if *previous == proxy => {},
            Some(_) => {
                #[cfg(feature = "tracing")]
                tracing::warn!("relays resolve to different proxies; connecting directly");
                return None;
            },
        }
    }
    let proxy = selected.flatten()?;
    if !matches!(proxy.scheme(), "http" | "https") {
        #[cfg(feature = "tracing")]
        tracing::warn!("relay connections cannot use SOCKS proxies; connecting directly");
        return None;
    }
    Some(proxy)
}

#[cfg(test)]
mod tests;
