// This is free and unencumbered software released into the public domain.

//! HTTP(S) execution of the JSON POST fetch/list protocol.
//!
//! [`FetcherRequest`] sends `{"urls":[URL,...],"options":{...}}` to `fetch`.
//! [`ListerRequest`] sends `{"url":URL,"options":{...}}` to `list`,
//! forwarding every configured [`ListerOptions`] field. Shared caching, filtering,
//! and timing fields are flattened into the same `options` object: `max-age`,
//! `jev`, `jq`, and `deadline`. Both operations use [`FilteringOptions`] for
//! filtering. Durations use human-readable strings such as `"1h"` or
//! `"1m 30s"`, preserving subsecond precision. The endpoint applies cache policy,
//! filters (Jev before jq), and relative deadlines.
//!
//! Sort and output-format values use their command-line spellings; `other` is an
//! array of literal arguments. Absent options and an empty `other` array are
//! omitted, as is the entire `options` object when empty. Explicit zero durations
//! and empty filter expressions are forwarded for endpoint validation. An endpoint
//! implementing this protocol must supply the RDF mapping profile expected by its
//! graph consumers; JSONL framing alone does not establish that mapping.
//!
//! ```no_run
//! use asimov_remote::{Execute, FetcherRequest};
//! use asimov_remote::http::{CachingOptions, Executor, Fetcher, Error};
//! use asimov_flow::StreamExt;
//! use core::time::Duration;
//!
//! # async fn example() -> Result<(), Error> {
//! let executor = Executor::new("https://asimov.social", None)?;
//! let caching = CachingOptions::builder()
//!     .max_age(Duration::from_secs(3600))
//!     .build();
//! let request = FetcherRequest::new(
//!     "https://example.com/profile", Default::default(),
//! ).with_caching(caching);
//! let mut fetcher = Fetcher::new(executor, request);
//! let mut batches = fetcher.execute().await?;
//! while let Some(batch) = batches.next().await {
//!     for line in batch?.lines() {
//!         // Consume or forward the original serialized bytes.
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use alloc::{boxed::Box, format, string::String};
use asimov_flow::{BatchStream, jsonl_batches_from_chunks};
use asimov_id::PublicKeyEncoding;
use asimov_keyring::Keyring;
use async_trait::async_trait;
use core::fmt;
use reqwest::{
    Client, Url,
    header::{ACCEPT, HeaderValue},
};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

pub use crate::{FetcherRequest, ListerRequest};
pub use asimov_flow::{BatchOptions, JsonlBatch, JsonlLine, StreamExt};
pub use asimov_patterns::{
    CachingOptions, FetcherOptions, FilteringOptions, ListerCapabilities, ListerOptions,
    OptionSupport, OutputFormat, TimingOptions,
};

/// A raw JSONL batch stream with HTTP-specific failures.
pub type JsonlStream = BatchStream<Error>;

/// Request configuration, startup, or response-body failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("failed to load local public key: {0}")]
    Keyring(#[from] asimov_keyring::KeyringError),
    #[error("invalid base URL: {0}")]
    InvalidBaseUrl(String),
    #[error("the HTTP fetch/list protocol does not support option {0}")]
    UnsupportedOption(&'static str),
}

/// Reusable HTTP transport configuration. Clones share the connection pool.
#[derive(Clone)]
pub struct Executor {
    client: Client,
    base_url: Url,
    api_token: SecretString,
    batching: BatchOptions,
}

impl fmt::Debug for Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpExecutor")
            .field("base_url", &self.base_url)
            .field("batching", &self.batching)
            .finish_non_exhaustive()
    }
}

impl Executor {
    /// Uses rustls with HTTP/2 negotiation and HTTP/1.1 fallback. Plain HTTP URLs
    /// are accepted for development. Path prefixes and an optional trailing slash
    /// are supported; query strings and fragments on the base URL are ignored.
    /// Sends an `Authorization: Bearer` header on every request. `Some(token)`
    /// uses the supplied token; `None` loads the current user's public key with
    /// [`Keyring::my_public_key`] and encodes it as Base58, without the non-ASCII
    /// ASIMOV prefix. The token is stored in a [`SecretString`], which zeroizes
    /// its storage on drop.
    ///
    /// The `User-Agent` is the caller name reported by `agent-detector` at
    /// construction, or `asimov-remote/<version>` if detection yields no valid
    /// HTTP header value.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid base URL, HTTP client setup failure, or
    /// failure to read or initialize the local identity when no token is given.
    pub fn new(base_url: impl AsRef<str>, api_token: Option<&str>) -> Result<Self, Error> {
        let mut base_url = Url::parse(base_url.as_ref())
            .map_err(|error| Error::InvalidBaseUrl(format!("{error}")))?;
        if !matches!(base_url.scheme(), "http" | "https") || base_url.host_str().is_none() {
            return Err(Error::InvalidBaseUrl(
                "expected an HTTP(S) URL with a host".into(),
            ));
        }
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        base_url.set_query(None);
        base_url.set_fragment(None);
        let user_agent = agent_detector::agent_name()
            .and_then(|name| HeaderValue::try_from(name).ok())
            .unwrap_or_else(|| {
                HeaderValue::from_static(concat!("asimov-remote/", env!("CARGO_PKG_VERSION")))
            });
        Ok(Self {
            client: Client::builder()
                .user_agent(user_agent)
                .use_rustls_tls()
                .build()?,
            base_url,
            api_token: match api_token {
                Some(token) => SecretString::from(token),
                None => Keyring::my_public_key()?
                    .encode(PublicKeyEncoding::Base58)
                    .expect("Base58 public-key encoding is always supported")
                    .into(),
            },
            batching: BatchOptions::default(),
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    #[must_use]
    pub fn with_batching(mut self, options: BatchOptions) -> Self {
        self.batching = options;
        self
    }

    /// POSTs a JSON request to a relative endpoint under this executor's base URL.
    /// Request/status errors are returned directly. Body errors terminate the
    /// returned stream after any complete buffered records. Bytes are not parsed
    /// as JSON or UTF-8. The stream owns the response, not a borrow of this executor.
    pub async fn post_jsonl(
        &self,
        endpoint: &str,
        request: &impl Serialize,
    ) -> Result<JsonlStream, Error> {
        // Endpoints are relative paths so the configured bearer token is always
        // sent to the configured service, including when a caller supplies a path.
        let url = self
            .base_url
            .join(endpoint)
            .map_err(|error| Error::InvalidBaseUrl(format!("{error}")))?;
        if url.origin() != self.base_url.origin() {
            return Err(Error::InvalidBaseUrl(
                "endpoint must use the configured origin".into(),
            ));
        }
        let response = self
            .client
            .post(url)
            .header(ACCEPT, "application/jsonl")
            .bearer_auth(self.api_token.expose_secret())
            .json(request)
            .send()
            .await?
            .error_for_status()?;
        Ok(jsonl_batches_from_chunks(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(Error::from)),
            self.batching,
        ))
    }
}

/// A configured resource fetch. Each execution sends its [`FetcherRequest`].
///
/// The endpoint applies caching, filtering (Jev before jq), and relative
/// execution deadlines. Only unset or `jsonl` output and an empty `other` array
/// are supported; other values are rejected before sending the request.
#[derive(Clone, Debug)]
pub struct Fetcher {
    executor: Executor,
    request: FetcherRequest,
}

impl Fetcher {
    /// Binds an owned request to an HTTP executor without sending it.
    pub fn new(executor: Executor, request: FetcherRequest) -> Self {
        Self { executor, request }
    }
}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Fetcher {
    type Error = Error;

    async fn execute(&mut self) -> Result<JsonlStream, Error> {
        let FetcherOptions { output, other } = &self.request.options;
        validate_fetcher_options(output.as_deref(), other)?;
        self.executor.post_jsonl("fetch", &self.request).await
    }
}

impl asimov_patterns::Fetcher<JsonlStream> for Fetcher {}

/// One configured listing. Offset and limit are native entry counts, delegated
/// to the endpoint. This implementation does not equate JSONL lines with entries
/// or impose a separate line cap. Every execution sends a fresh request,
/// including when the limit is zero. The endpoint validates and applies options.
///
/// [`ListerRequest`] defines the body and its option encoding. The endpoint
/// applies caching, filtering (Jev before jq), and relative execution deadlines.
///
/// `T` is the sort-key type and `F` the custom output-format type from
/// [`ListerOptions<T, F>`], both defaulting to [`String`]. Execution uses
/// [`fmt::Display`] for both key and custom format names. No parsing or Serde
/// traits are required on either type. Responses retain their original bytes
/// in line-delimited batches; their format is not validated or transcoded.
/// Both types must be [`Send`] and [`Sync`] for the sendable execution future.
/// Use a `Lister` type annotation or `Lister::<String>::new` when the options
/// do not otherwise determine the generic types.
#[derive(Clone, Debug)]
pub struct Lister<T: Clone = String, F = String> {
    executor: Executor,
    request: ListerRequest<T, F>,
}

impl<T: Clone, F> Lister<T, F> {
    /// Binds an owned request with typed options to an HTTP executor.
    pub fn new(executor: Executor, request: ListerRequest<T, F>) -> Self {
        Self { executor, request }
    }

    /// Operations this HTTP request schema can forward.
    ///
    /// These describe transport support, not dynamically discovered endpoint
    /// capabilities. The endpoint determines which requests it can execute.
    pub fn capabilities(&self) -> ListerCapabilities {
        ListerCapabilities {
            sort: OptionSupport::Supported,
            before: OptionSupport::Supported,
            after: OptionSupport::Supported,
            offset: OptionSupport::Supported,
            limit: OptionSupport::Supported,
        }
    }
}

#[async_trait]
impl<T: Clone + fmt::Display + Send + Sync, F: fmt::Display + Send + Sync>
    asimov_patterns::Execute<JsonlStream> for Lister<T, F>
{
    type Error = Error;

    async fn execute(&mut self) -> Result<JsonlStream, Error> {
        self.executor.post_jsonl("list", &self.request).await
    }
}

impl<T: Clone + fmt::Display + Send + Sync, F: fmt::Display + Send + Sync>
    asimov_patterns::Lister<JsonlStream> for Lister<T, F>
{
}

fn validate_fetcher_options(output: Option<&str>, other: &[String]) -> Result<(), Error> {
    if output.is_some_and(|format| format != "jsonl") {
        return Err(Error::UnsupportedOption("output"));
    }
    if !other.is_empty() {
        return Err(Error::UnsupportedOption("other"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
