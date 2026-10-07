// This is free and unencumbered software released into the public domain.

//! HTTP(S) execution of the JSON POST fetch/list protocol.
//!
//! Fetching maps one resource URL to `{"urls":[URL],"options":{...}}` at `fetch`.
//! Listing maps a collection URL to `{"url":URL,"options":{...}}` at `list`,
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
//! use asimov_remote::{Execute, http::{CachingOptions, Executor, Fetcher, Error}};
//! use asimov_flow::StreamExt;
//! use core::time::Duration;
//!
//! # async fn example() -> Result<(), Error> {
//! let executor = Executor::new("https://asimov.social", Some("api-token"))?;
//! let caching = CachingOptions::builder()
//!     .max_age(Duration::from_secs(3600))
//!     .build();
//! let mut fetcher = Fetcher::new(
//!     executor, "https://example.com/profile", Default::default(),
//! ).with_caching(caching);
//! let mut batches = fetcher.execute().await?;
//! while let Some(batch) = batches.next().await {
//!     for line in batch?.lines() {
//!         // Consume or forward the original serialized bytes.
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};
use asimov_flow::{BatchStream, jsonl_batches_from_chunks};
use async_trait::async_trait;
use core::fmt;
use reqwest::{Client, Url, header::ACCEPT};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

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
    api_token: Option<SecretString>,
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
    /// Pass `Some(token)` to send an `Authorization: Bearer` header, or `None`
    /// to omit authentication. The token is copied into a [`SecretString`],
    /// which zeroizes its storage on drop.
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
        Ok(Self {
            client: Client::builder().use_rustls_tls().build()?,
            base_url,
            api_token: api_token.map(SecretString::from),
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
        let mut request = self
            .client
            .post(url)
            .header(ACCEPT, "application/jsonl")
            .json(request);
        if let Some(token) = &self.api_token {
            request = request.bearer_auth(token.expose_secret());
        }
        let response = request.send().await?.error_for_status()?;
        Ok(jsonl_batches_from_chunks(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(Error::from)),
            self.batching,
        ))
    }
}

/// One configured resource fetch. Each execution sends a fresh request.
///
/// [`CachingOptions`], [`FilteringOptions`], and [`TimingOptions`] are flattened
/// into the request's `options` object as `max-age`, `jev`, `jq`, and `deadline`.
/// Durations use human-readable strings. The endpoint applies these options,
/// including Jev filtering before jq and relative execution deadlines. Unset
/// fields are omitted. Only unset or `jsonl` output and an empty `other` array
/// are supported.
#[derive(Clone, Debug)]
pub struct Fetcher {
    executor: Executor,
    input: String,
    options: FetcherOptions,
    caching: CachingOptions,
    filtering: FilteringOptions,
    timing: TimingOptions,
}

impl Fetcher {
    pub fn new(executor: Executor, input: impl Into<String>, options: FetcherOptions) -> Self {
        Self {
            executor,
            input: input.into(),
            options,
            caching: CachingOptions::default(),
            filtering: FilteringOptions::default(),
            timing: TimingOptions::default(),
        }
    }

    /// Sets cache freshness options to forward to the endpoint.
    #[must_use]
    pub fn with_caching(mut self, options: CachingOptions) -> Self {
        self.caching = options;
        self
    }

    /// Sets output filtering options to forward to the endpoint.
    #[must_use]
    pub fn with_filtering(mut self, options: FilteringOptions) -> Self {
        self.filtering = options;
        self
    }

    /// Sets execution timing options to forward to the endpoint.
    #[must_use]
    pub fn with_timing(mut self, options: TimingOptions) -> Self {
        self.timing = options;
        self
    }
}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Fetcher {
    type Error = Error;

    async fn execute(&mut self) -> Result<JsonlStream, Error> {
        let FetcherOptions { output, other } = &self.options;
        validate_fetcher_options(output.as_deref(), other)?;
        #[derive(Serialize)]
        struct Request<'a> {
            urls: [&'a str; 1],
            #[serde(skip_serializing_if = "SharedOptions::is_empty")]
            options: SharedOptions<'a>,
        }
        self.executor
            .post_jsonl(
                "fetch",
                &Request {
                    urls: [&self.input],
                    options: SharedOptions::new(&self.caching, &self.filtering, &self.timing),
                },
            )
            .await
    }
}

impl asimov_patterns::Fetcher<JsonlStream> for Fetcher {}

/// One configured listing. Offset and limit are native entry counts, delegated
/// to the endpoint. This implementation does not equate JSONL lines with entries
/// or impose a separate line cap. Every execution sends a fresh request,
/// including when the limit is zero. The endpoint validates and applies options.
///
/// All [`ListerOptions`] fields are forwarded in the request's `options` object:
/// `sort`, `before`, `after`, `offset`, `limit`, `output`, and `other`. Sort keys
/// are comma-separated with a `-` prefix for descending order. Formats use
/// `jsonl`, `url`, or the custom format's name. Cursors and literal `other`
/// arguments retain their spelling and argument boundaries. Unset fields and
/// an empty `other` array are omitted; explicit zero values are preserved.
/// [`CachingOptions`], [`FilteringOptions`], and [`TimingOptions`] are flattened
/// into this object as `max-age`, `jev`, `jq`, and `deadline`. Durations use
/// human-readable strings. The endpoint applies these options, including Jev
/// filtering before jq and relative execution deadlines.
///
/// `T` is the sort-key type and `F` the custom output-format type from
/// [`ListerOptions<T, F>`], both defaulting to [`String`]. Execution uses
/// [`fmt::Display`] for both key and custom format names. No parsing or Serde
/// traits are required on either type. Responses retain their original bytes
/// in line-delimited batches; their format is not validated or transcoded.
/// Both types must be [`Send`] for the execution trait's sendable future.
/// Use a `Lister` type annotation or `Lister::<String>::new` when the options
/// do not otherwise determine the generic types.
#[derive(Clone, Debug)]
pub struct Lister<T: Clone = String, F = String> {
    executor: Executor,
    input: String,
    options: ListerOptions<T, F>,
    caching: CachingOptions,
    filtering: FilteringOptions,
    timing: TimingOptions,
}

impl<T: Clone, F> Lister<T, F> {
    /// Configures a listing with typed options without sending a request.
    pub fn new(executor: Executor, input: impl Into<String>, options: ListerOptions<T, F>) -> Self {
        Self {
            executor,
            input: input.into(),
            options,
            caching: CachingOptions::default(),
            filtering: FilteringOptions::default(),
            timing: TimingOptions::default(),
        }
    }

    /// Sets cache freshness options to forward to the endpoint.
    #[must_use]
    pub fn with_caching(mut self, options: CachingOptions) -> Self {
        self.caching = options;
        self
    }

    /// Sets output filtering options to forward to the endpoint.
    #[must_use]
    pub fn with_filtering(mut self, options: FilteringOptions) -> Self {
        self.filtering = options;
        self
    }

    /// Sets execution timing options to forward to the endpoint.
    #[must_use]
    pub fn with_timing(mut self, options: TimingOptions) -> Self {
        self.timing = options;
        self
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
impl<T: Clone + fmt::Display + Send, F: fmt::Display + Send> asimov_patterns::Execute<JsonlStream>
    for Lister<T, F>
{
    type Error = Error;

    async fn execute(&mut self) -> Result<JsonlStream, Error> {
        #[derive(Serialize)]
        struct Options<'a> {
            #[serde(flatten)]
            shared: SharedOptions<'a>,
            #[serde(skip_serializing_if = "Option::is_none")]
            sort: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            before: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            after: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            offset: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            output: Option<String>,
            #[serde(skip_serializing_if = "<[String]>::is_empty")]
            other: &'a [String],
        }
        #[derive(Serialize)]
        struct Request<'a> {
            url: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            options: Option<Options<'a>>,
        }
        let options = {
            // Exhaustive destructuring makes newly added option fields a
            // compile error here until their wire representation is supplied.
            let ListerOptions {
                sort,
                before,
                after,
                offset,
                limit,
                output,
                other,
            } = &self.options;
            let shared = SharedOptions::new(&self.caching, &self.filtering, &self.timing);
            let empty = sort.is_none()
                && before.is_none()
                && after.is_none()
                && offset.is_none()
                && limit.is_none()
                && output.is_none()
                && other.is_empty()
                && shared.is_empty();
            (!empty).then(|| Options {
                shared,
                sort: sort.as_ref().map(|keys| {
                    keys.keys()
                        .iter()
                        .map(|key| {
                            format!("{}{}", if key.descending() { "-" } else { "" }, key.key())
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                }),
                before: before.as_deref(),
                after: after.as_deref(),
                offset: *offset,
                limit: *limit,
                output: output.as_ref().map(ToString::to_string),
                other,
            })
        };
        self.executor
            .post_jsonl(
                "list",
                &Request {
                    url: &self.input,
                    options,
                },
            )
            .await
    }
}

impl<T: Clone + fmt::Display + Send, F: fmt::Display + Send> asimov_patterns::Lister<JsonlStream>
    for Lister<T, F>
{
}

#[derive(Serialize)]
struct SharedOptions<'a> {
    #[serde(rename = "max-age", skip_serializing_if = "Option::is_none")]
    max_age: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jev: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    jq: Option<&'a str>,
}

impl<'a> SharedOptions<'a> {
    fn new(
        caching: &CachingOptions,
        filtering: &'a FilteringOptions,
        timing: &TimingOptions,
    ) -> Self {
        let CachingOptions { max_age } = caching;
        let FilteringOptions { jev, jq } = filtering;
        let TimingOptions { deadline } = timing;
        Self {
            max_age: max_age.map(|value| humantime::format_duration(value).to_string()),
            deadline: deadline.map(|value| humantime::format_duration(value).to_string()),
            jev: jev.as_deref(),
            jq: jq.as_deref(),
        }
    }

    fn is_empty(&self) -> bool {
        self.max_age.is_none() && self.deadline.is_none() && self.jev.is_none() && self.jq.is_none()
    }
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
