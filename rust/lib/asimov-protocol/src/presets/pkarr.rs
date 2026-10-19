// This is free and unencumbered software released into the public domain.

//! Pkarr's wire format stays with Iroh; HTTP routing belongs to the SDK.

use alloc::{boxed::Box, string::ToString, vec::Vec};
use asimov_http::proxy::{OriginProxy, ProxyOptions};
use core::{fmt, pin::Pin, time::Duration};
use futures_lite::Stream;
use iroh::{
    Endpoint, EndpointId, SecretKey,
    address_lookup::{
        self, AddrFilter, AddressLookup, AddressLookupBuilder, AddressLookupBuilderError,
        DEFAULT_PKARR_TTL, DEFAULT_REPUBLISH_INTERVAL, EndpointData, EndpointInfo, Item,
        N0_DNS_PKARR_RELAY_PROD, N0_DNS_PKARR_RELAY_STAGING,
    },
};
use iroh_dns::pkarr::SignedPacket;
use reqwest::{Method, Response, StatusCode, Url};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Debug)]
pub(super) struct Builder {
    url: Url,
}

impl Default for Builder {
    fn default() -> Self {
        let url = if iroh::endpoint::force_staging_infra() {
            N0_DNS_PKARR_RELAY_STAGING
        } else {
            N0_DNS_PKARR_RELAY_PROD
        };
        Self {
            url: url.parse().expect("static Pkarr URL"),
        }
    }
}

impl AddressLookupBuilder for Builder {
    fn into_address_lookup(
        self,
        endpoint: &Endpoint,
    ) -> Result<impl AddressLookup, AddressLookupBuilderError> {
        let client = Client::new(self.url, endpoint)
            .map_err(|err| AddressLookupBuilderError::from_err("pkarr", err))?;
        Ok(Lookup::new(
            client,
            endpoint.secret_key().clone(),
            DEFAULT_REPUBLISH_INTERVAL,
        ))
    }
}

#[derive(Debug)]
struct Lookup {
    client: Client,
    endpoint_id: EndpointId,
    updates: watch::Sender<Option<EndpointInfo>>,
    publisher: JoinHandle<()>,
}

impl Lookup {
    fn new(client: Client, key: SecretKey, republish_interval: Duration) -> Self {
        let (updates, receiver) = watch::channel(None);
        let endpoint_id = key.public();
        let publisher = tokio::spawn(publish_loop(
            client.clone(),
            key,
            receiver,
            republish_interval,
        ));
        Self {
            client,
            endpoint_id,
            updates,
            publisher,
        }
    }
}

impl Drop for Lookup {
    fn drop(&mut self) {
        self.publisher.abort();
    }
}

impl AddressLookup for Lookup {
    fn publish(&self, data: &EndpointData) {
        let data = data.apply_filter(&AddrFilter::relay_only()).into_owned();
        let info = EndpointInfo::from_parts(self.endpoint_id, data);
        self.updates.send_if_modified(|current| {
            if current.as_ref() == Some(&info) {
                return false;
            }
            *current = Some(info);
            true
        });
    }

    fn resolve(
        &self,
        endpoint_id: EndpointId,
    ) -> Option<Pin<Box<dyn Stream<Item = Result<Item, address_lookup::Error>> + Send>>> {
        let client = self.client.clone();
        Some(Box::pin(async_stream::stream! {
            yield client.resolve(endpoint_id).await
                .map_err(|err| address_lookup::Error::from_err("pkarr", err));
        }))
    }
}

async fn publish_loop(
    client: Client,
    key: SecretKey,
    mut updates: watch::Receiver<Option<EndpointInfo>>,
    republish_interval: Duration,
) {
    let mut failures = 0u64;
    loop {
        let current = updates.borrow_and_update().clone();
        let Some(info) = current else {
            if updates.changed().await.is_err() {
                break;
            }
            continue;
        };
        let result = async {
            let packet = info.to_pkarr_signed_packet(&key, DEFAULT_PKARR_TTL)?;
            client.publish(&packet).await
        }
        .await;
        let delay = match result {
            Ok(()) => {
                failures = 0;
                republish_interval
            },
            Err(_error) => {
                #[cfg(feature = "tracing")]
                tracing::warn!(error = %_error, "failed to publish Pkarr address record");
                failures = failures.saturating_add(1);
                Duration::from_secs(failures.min(300))
            },
        };
        tokio::select! {
            changed = updates.changed() => if changed.is_err() { break; },
            _ = tokio::time::sleep(delay) => {},
        }
    }
}

#[derive(Clone)]
struct Client {
    url: Url,
    proxy: OriginProxy,
    http: reqwest::Client,
    tls: rustls::ClientConfig,
    dns: iroh::dns::DnsResolver,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PkarrClient")
            .field("origin", self.proxy.origin())
            .finish_non_exhaustive()
    }
}

impl Client {
    fn new(url: Url, endpoint: &Endpoint) -> Result<Self, Error> {
        validate_url(&url)?;
        let proxy = OriginProxy::resolve(&url, &ProxyOptions::default())?;
        let tls = endpoint.tls_config().clone();
        let dns = endpoint.dns_resolver()?.clone();
        let http = Self::build_http(&proxy, &tls, &dns)?;
        Ok(Self {
            url,
            proxy,
            http,
            tls,
            dns,
        })
    }

    fn build_http(
        proxy: &OriginProxy,
        tls: &rustls::ClientConfig,
        dns: &iroh::dns::DnsResolver,
    ) -> Result<reqwest::Client, Error> {
        let builder = reqwest::Client::builder()
            .user_agent("ASIMOV.rs")
            .tls_backend_preconfigured(tls.clone())
            .dns_resolver(DnsResolver(dns.clone()))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30));
        Ok(proxy.configure_reqwest(builder)?.build()?)
    }

    async fn request(
        &self,
        method: Method,
        id: EndpointId,
        body: Option<Vec<u8>>,
    ) -> Result<Response, Error> {
        let mut url = self.url.clone();
        url.path_segments_mut()
            .expect("HTTP URL")
            .pop_if_empty()
            .push(&id.to_z32());
        for redirects in 0..=10 {
            validate_url(&url)?;
            let client = if self.proxy.check_origin(&url).is_ok() {
                self.http.clone()
            } else {
                let proxy = OriginProxy::resolve(&url, &ProxyOptions::default())?;
                Self::build_http(&proxy, &self.tls, &self.dns)?
            };
            let mut request = client.request(method.clone(), url.clone());
            if let Some(body) = &body {
                request = request.body(body.clone());
            }
            let response = request.send().await?;
            if matches!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            ) {
                if redirects == 10 {
                    return Err(Error::InvalidResponse("too many redirects"));
                }
                if method == Method::PUT
                    && !matches!(
                        response.status(),
                        StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT
                    )
                {
                    return Err(Error::InvalidResponse(
                        "publication requires a method-preserving redirect",
                    ));
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or(Error::InvalidResponse(
                        "missing or invalid redirect Location",
                    ))?;
                let next = url
                    .join(location)
                    .map_err(|_| Error::InvalidResponse("invalid redirect URL"))?;
                if url.scheme() == "https" && next.scheme() != "https" {
                    return Err(Error::InvalidResponse("HTTPS redirect downgrade"));
                }
                url = next;
                continue;
            }
            if !response.status().is_success() {
                return Err(Error::Status(response.status()));
            }
            return Ok(response);
        }
        unreachable!("redirect limit checked before following")
    }

    async fn resolve(&self, id: EndpointId) -> Result<Item, Error> {
        let mut response = self.request(Method::GET, id, None).await?;
        // Relay payloads omit the 32-byte public key from a signed packet.
        const MAX_PAYLOAD: usize = SignedPacket::MAX_BYTES - 32;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_PAYLOAD as u64)
        {
            return Err(Error::InvalidResponse("Pkarr record is too large"));
        }
        let mut payload = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if chunk.len() > MAX_PAYLOAD - payload.len() {
                return Err(Error::InvalidResponse("Pkarr record is too large"));
            }
            payload.extend_from_slice(&chunk);
        }
        let packet = SignedPacket::from_relay_payload(&id, &payload)?;
        let info = EndpointInfo::from_pkarr_signed_packet(&packet)?;
        Ok(Item::new(info, "pkarr", None))
    }

    async fn publish(&self, packet: &SignedPacket) -> Result<(), Error> {
        self.request(
            Method::PUT,
            packet.public_key(),
            Some(packet.to_relay_payload()),
        )
        .await?;
        Ok(())
    }
}

fn validate_url(url: &Url) -> Result<(), Error> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidResponse(
            "expected an HTTP(S) URL without userinfo or fragment",
        ));
    }
    Ok(())
}

struct DnsResolver(iroh::dns::DnsResolver);

impl reqwest::dns::Resolve for DnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = self.0.clone();
        let name = name.as_str().to_string();
        Box::pin(async move {
            let addresses = resolver
                .lookup_ipv4_ipv6_staggered(
                    // Match Iroh's staggered IPv4/IPv6 lookup schedule.
                    name,
                    iroh::dns::DNS_TIMEOUT,
                    &[200, 300, 600, 1000, 2000, 3000],
                )
                .await?
                .map(|ip| std::net::SocketAddr::new(ip, 0))
                .collect::<Vec<_>>();
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Proxy(#[from] asimov_http::proxy::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("Pkarr HTTP status: {0}")]
    Status(StatusCode),
    #[error("invalid Pkarr HTTP response: {0}")]
    InvalidResponse(&'static str),
    #[error(transparent)]
    Endpoint(#[from] iroh::endpoint::EndpointError),
    #[error(transparent)]
    Encoding(#[from] iroh_dns::EncodingError),
    #[error(transparent)]
    Packet(#[from] iroh_dns::pkarr::SignedPacketVerifyError),
    #[error(transparent)]
    Parse(#[from] iroh_dns::ParseError),
}

#[cfg(test)]
mod tests;
