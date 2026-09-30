// This is free and unencumbered software released into the public domain.

//! HTTP(S) execution of the JSON POST fetch/list protocol.
//!
//! Fetching maps one resource URL to `{"urls":[URL]}` at `fetch`. Listing maps
//! a collection URL to `{"url":URL,"options":{...}}` at `list`, forwarding
//! every configured [`ListerOptions`] field. Sort and output-format values use
//! their command-line spellings; `other` is an array of literal arguments.
//! Absent options and an empty `other` array are omitted, as is the entire
//! `options` object when empty. An endpoint implementing this protocol must
//! supply the RDF mapping profile expected by its graph consumers; JSONL framing
//! alone does not establish that mapping.
//!
//! ```no_run
//! use asimov_remote::{Execute, http::{Executor, Fetcher, Error}};
//! use asimov_flow::StreamExt;
//!
//! # async fn example() -> Result<(), Error> {
//! let executor = Executor::new("https://asimov.social", "api-token")?;
//! let mut fetcher = Fetcher::new(executor, "https://example.com/profile", Default::default());
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
use serde::Serialize;

pub use asimov_flow::{BatchOptions, JsonlBatch, JsonlLine, StreamExt};
pub use asimov_patterns::{
    FetcherOptions, ListerCapabilities, ListerOptions, OptionSupport, OutputFormat,
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
    api_token: String,
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
    pub fn new(base_url: impl AsRef<str>, api_token: impl Into<String>) -> Result<Self, Error> {
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
            api_token: api_token.into(),
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
            .bearer_auth(&self.api_token)
            .header(ACCEPT, "application/jsonl")
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

/// One configured resource fetch. Each execution sends a fresh request.
#[derive(Clone, Debug)]
pub struct Fetcher {
    executor: Executor,
    input: String,
    options: FetcherOptions,
}

impl Fetcher {
    pub fn new(executor: Executor, input: impl Into<String>, options: FetcherOptions) -> Self {
        Self {
            executor,
            input: input.into(),
            options,
        }
    }
}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Fetcher {
    type Error = Error;

    async fn execute(&mut self) -> Result<JsonlStream, Error> {
        validate_fetcher_options(self.options.output.as_deref(), &self.options.other)?;
        #[derive(Serialize)]
        struct Request<'a> {
            urls: [&'a str; 1],
        }
        self.executor
            .post_jsonl(
                "fetch",
                &Request {
                    urls: [&self.input],
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
}

impl<T: Clone, F> Lister<T, F> {
    /// Configures a listing with typed options without sending a request.
    pub fn new(executor: Executor, input: impl Into<String>, options: ListerOptions<T, F>) -> Self {
        Self {
            executor,
            input: input.into(),
            options,
        }
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
            let empty = sort.is_none()
                && before.is_none()
                && after.is_none()
                && offset.is_none()
                && limit.is_none()
                && output.is_none()
                && other.is_empty();
            (!empty).then(|| Options {
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
