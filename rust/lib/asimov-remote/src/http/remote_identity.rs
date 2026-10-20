// This is free and unencumbered software released into the public domain.

//! Private, origin-scoped discovery using the executor's connection pool.

use super::{Error, PublicKey, StreamExt, TimeoutOptions, TimeoutPhase, Verifier, timeouts};
use alloc::{boxed::Box, format, string::String, sync::Arc, vec, vec::Vec};
use asimov_http::{Component, VerificationPolicy, VerifyingKey, key_id};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use core::time::Duration;
use reqwest::{
    Client, StatusCode, Url,
    header::{self, HeaderValue},
};
use serde::Deserialize;
use tokio::{
    sync::{Mutex, OnceCell},
    time::Instant,
};

const MAX_BYTES: usize = 64 * 1024;
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(300);

type Failure = dyn core::error::Error + Send + Sync;
type LookupResult = Result<Arc<CachedIdentity>, Arc<Failure>>;

#[derive(Default)]
pub(super) struct RemoteIdentityCache {
    lookup: Mutex<Option<Arc<Lookup>>>,
}

struct Lookup {
    result: OnceCell<LookupResult>,
    previous: Option<Arc<CachedIdentity>>,
}

struct CachedIdentity {
    public_key: PublicKey,
    verifier: Verifier,
    check_verifier: Verifier,
    etag: Option<HeaderValue>,
    cache_control: Option<String>,
    received: Instant,
    fresh_for: Duration,
    no_store: bool,
}

impl RemoteIdentityCache {
    pub(super) async fn public_key(
        &self,
        client: &Client,
        url: &Url,
        timeouts: TimeoutOptions,
    ) -> Result<PublicKey, Error> {
        Ok(self.get(client, url, timeouts).await?.public_key)
    }

    pub(super) async fn verifier(
        &self,
        client: &Client,
        url: &Url,
        timeouts: TimeoutOptions,
    ) -> Result<Verifier, Error> {
        Ok(self.get(client, url, timeouts).await?.verifier.clone())
    }

    pub(super) async fn check_verifier(
        &self,
        client: &Client,
        url: &Url,
        timeouts: TimeoutOptions,
    ) -> Result<Verifier, Error> {
        Ok(self
            .get(client, url, timeouts)
            .await?
            .check_verifier
            .clone())
    }

    async fn get(
        &self,
        client: &Client,
        url: &Url,
        timeouts: TimeoutOptions,
    ) -> Result<Arc<CachedIdentity>, Error> {
        timeouts::limit(
            timeouts.discovery,
            TimeoutPhase::Discovery,
            self.get_inner(client, url, timeouts),
        )
        .await
        .map_err(|error| Error::RemoteKey(Arc::new(error)))?
    }

    async fn get_inner(
        &self,
        client: &Client,
        url: &Url,
        timeouts: TimeoutOptions,
    ) -> Result<Arc<CachedIdentity>, Error> {
        let lookup = {
            let mut slot = self.lookup.lock().await;
            let previous = match slot.as_ref().and_then(|lookup| lookup.result.get()) {
                None => None,
                Some(Ok(cached)) if cached.received.elapsed() < cached.fresh_for => {
                    return Ok(cached.clone());
                },
                Some(result) => {
                    let previous = match result {
                        Ok(cached) if !cached.no_store => Some(cached.clone()),
                        Ok(_) => None,
                        Err(_) => slot.as_ref().and_then(|lookup| lookup.previous.clone()),
                    };
                    *slot = None;
                    previous
                },
            };
            slot.get_or_insert_with(|| {
                Arc::new(Lookup {
                    result: OnceCell::new(),
                    previous,
                })
            })
            .clone()
        };
        // A cancelled initializer leaves the cell available for another waiter.
        // Failures are shared by waiters but retried on the next lookup.
        let result = lookup
            .result
            .get_or_init(|| async {
                discover(client, url, lookup.previous.as_deref(), timeouts)
                    .await
                    .map(Arc::new)
                    .map_err(Arc::from)
            })
            .await;
        if result.as_ref().is_ok_and(|cached| cached.no_store) {
            let mut slot = self.lookup.lock().await;
            if slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &lookup))
            {
                *slot = None;
            }
        }
        result.clone().map_err(Error::RemoteKey)
    }
}

async fn discover(
    client: &Client,
    base_url: &Url,
    previous: Option<&CachedIdentity>,
    timeouts: TimeoutOptions,
) -> Result<CachedIdentity, Box<Failure>> {
    // Origin serialization discards path prefixes and URL credentials.
    let url = format!(
        "{}/.well-known/jwks.json",
        base_url.origin().ascii_serialization()
    );
    let mut request = client.get(url).header(header::ACCEPT, "application/json");
    if let Some(etag) = previous.and_then(|cached| cached.etag.as_ref()) {
        request = request.header(header::IF_NONE_MATCH, etag.clone());
    }
    let started = Instant::now();
    let response = timeouts::limit(
        timeouts.response_headers,
        TimeoutPhase::ResponseHeaders,
        request.send(),
    )
    .await??;
    super::rate_limit::check(&response)?;
    let unchanged = response.status() == StatusCode::NOT_MODIFIED;
    if !response.status().is_success() && !unchanged {
        return Err(format!(
            "remote identity endpoint returned HTTP {}",
            response.status()
        )
        .into());
    }
    if unchanged && previous.and_then(|cached| cached.etag.as_ref()).is_none() {
        return Err("unsolicited remote identity revalidation response".into());
    }
    let cache_control = if response.headers().contains_key(header::CACHE_CONTROL) {
        Some(
            response
                .headers()
                .get_all(header::CACHE_CONTROL)
                .iter()
                .map(|value| value.to_str())
                .collect::<Result<Vec<_>, _>>()?
                .join(","),
        )
    } else if unchanged {
        previous.and_then(|cached| cached.cache_control.clone())
    } else {
        None
    };
    let etag = response.headers().get(header::ETAG).cloned().or_else(|| {
        if unchanged {
            previous.and_then(|cached| cached.etag.clone())
        } else {
            None
        }
    });
    let age = match response.headers().get(header::AGE) {
        Some(value) => value
            .to_str()
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(u64::MAX),
        None => 0,
    };
    if response
        .content_length()
        .is_some_and(|size| size > MAX_BYTES as u64)
    {
        return Err("remote identity response exceeds 64 KiB".into());
    }
    let mut bytes = Vec::new();
    let mut chunks = timeouts::TimedStream::new(
        response
            .bytes_stream()
            .map(|chunk| chunk.map_err(Error::from)),
        timeouts.body_idle,
        None,
    );
    // Consume to EOF so HTTP/1.1 can reuse this connection for the operation.
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk?;
        if chunk.len() > MAX_BYTES - bytes.len() {
            return Err("remote identity response exceeds 64 KiB".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let (public_key, verifier, check_verifier) = if unchanged {
        let cached = previous.expect("validated conditional response");
        (
            cached.public_key,
            cached.verifier.clone(),
            cached.check_verifier.clone(),
        )
    } else {
        parse_identity(&bytes)?
    };
    let (max_age, no_store) = cache_policy(cache_control.as_deref());
    Ok(CachedIdentity {
        public_key,
        verifier,
        check_verifier,
        etag,
        cache_control,
        received: Instant::now(),
        fresh_for: max_age
            .saturating_sub(Duration::from_secs(age))
            .saturating_sub(started.elapsed()),
        no_store,
    })
}

// A small private freshness policy, not a general-purpose HTTP response cache.
// Invalid or conflicting max-age directives conservatively force revalidation.
fn cache_policy(control: Option<&str>) -> (Duration, bool) {
    let mut max_age = None;
    let mut revalidate = false;
    let mut no_store = false;
    for directive in control.unwrap_or_default().split(',') {
        let (name, value) = directive
            .trim()
            .split_once('=')
            .unwrap_or((directive.trim(), ""));
        if name.trim().eq_ignore_ascii_case("no-store") {
            no_store = true;
            revalidate = true;
        } else if name.trim().eq_ignore_ascii_case("no-cache") {
            revalidate = true;
        } else if name.trim().eq_ignore_ascii_case("max-age") {
            if max_age.is_some() {
                revalidate = true;
            }
            max_age = value
                .trim()
                .trim_matches('"')
                .parse::<u64>()
                .ok()
                .map(Duration::from_secs);
            revalidate |= max_age.is_none();
        }
    }
    (
        if revalidate {
            Duration::ZERO
        } else {
            max_age.unwrap_or(DEFAULT_MAX_AGE)
        },
        no_store,
    )
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kty: String,
    crv: Option<String>,
    x: Option<String>,
    kid: Option<String>,
    alg: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
}

fn parse_identity(bytes: &[u8]) -> Result<(PublicKey, Verifier, Verifier), Box<Failure>> {
    let jwks: Jwks = serde_json::from_slice(bytes)?;
    let mut keys = jwks.keys.into_iter().filter(|key| {
        key.kty == "OKP"
            && key.crv.as_deref() == Some("Ed25519")
            && key
                .alg
                .as_deref()
                .is_none_or(|alg| matches!(alg, "EdDSA" | "Ed25519"))
            && key.usage.as_deref().is_none_or(|usage| usage == "sig")
            && key
                .key_ops
                .as_ref()
                .is_none_or(|ops| ops.iter().any(|op| op == "verify"))
    });
    let jwk = keys
        .next()
        .ok_or("remote identity contains no Ed25519 verification key")?;
    if keys.next().is_some() {
        return Err("remote identity contains multiple Ed25519 verification keys".into());
    }
    let bytes: [u8; 32] = URL_SAFE_NO_PAD
        .decode(jwk.x.ok_or("missing public key (x)")?)?
        .try_into()
        .map_err(|_| "Ed25519 public key must contain 32 bytes")?;
    let key = VerifyingKey::from_bytes(&bytes)?;
    let id = jwk.kid.unwrap_or_else(|| key_id(&key));
    let policy = VerificationPolicy::new(vec![
        Component::new("@status")?,
        Component::new("content-type")?,
    ]);
    Ok((
        PublicKey::from(bytes),
        Verifier::new(key, &id, "asimov", policy)?,
        Verifier::new(
            key,
            &id,
            "asimov",
            VerificationPolicy::new(vec![Component::new("@status")?]),
        )?,
    ))
}
