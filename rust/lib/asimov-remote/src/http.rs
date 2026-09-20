// This is free and unencumbered software released into the public domain.

//! HTTP(S) execution of the JSON POST fetch/list protocol.
//!
//! Fetching maps one resource URL to `{"urls":[URL]}` at `fetch`. Listing maps
//! a collection URL to `{"url":URL,"options":{"offset":N,"limit":N}}` at
//! `list`, omitting absent options. An endpoint implementing this protocol must
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

use alloc::{boxed::Box, format, string::String};
use asimov_flow::{BatchStream, jsonl_batches_from_chunks};
use async_trait::async_trait;
use core::fmt;
use reqwest::{Client, Url, header::ACCEPT};
use serde::Serialize;

pub use asimov_flow::{BatchOptions, JsonlBatch, JsonlLine, StreamExt};
pub use asimov_patterns::{FetcherOptions, ListerCapabilities, ListerOptions, OptionSupport};

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
        validate_common(self.options.output.as_deref(), &self.options.other)?;
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
/// or impose a separate line cap. Each execution sends a fresh request; a zero
/// limit returns an empty stream after validation without issuing a request.
#[derive(Clone, Debug)]
pub struct Lister {
    executor: Executor,
    input: String,
    options: ListerOptions,
}

impl Lister {
    pub fn new(executor: Executor, input: impl Into<String>, options: ListerOptions) -> Self {
        Self {
            executor,
            input: input.into(),
            options,
        }
    }

    /// Capabilities of this HTTP request schema, not dynamically discovered metadata.
    pub fn capabilities(&self) -> ListerCapabilities {
        ListerCapabilities {
            sort: OptionSupport::Unsupported,
            before: OptionSupport::Unsupported,
            after: OptionSupport::Unsupported,
            offset: OptionSupport::Supported,
            limit: OptionSupport::Supported,
        }
    }
}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Lister {
    type Error = Error;

    async fn execute(&mut self) -> Result<JsonlStream, Error> {
        validate_common(self.options.output.as_deref(), &self.options.other)?;
        for (option, requested) in [
            ("sort", self.options.sort.is_some()),
            ("before", self.options.before.is_some()),
            ("after", self.options.after.is_some()),
        ] {
            if requested {
                return Err(Error::UnsupportedOption(option));
            }
        }
        if self.options.limit == Some(0) {
            return Ok(Box::pin(asimov_flow::stream::empty()));
        }
        #[derive(Serialize)]
        struct Options {
            #[serde(skip_serializing_if = "Option::is_none")]
            offset: Option<usize>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<usize>,
        }
        #[derive(Serialize)]
        struct Request<'a> {
            url: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            options: Option<Options>,
        }
        let options =
            (self.options.offset.is_some() || self.options.limit.is_some()).then_some(Options {
                offset: self.options.offset,
                limit: self.options.limit,
            });
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

impl asimov_patterns::Lister<JsonlStream> for Lister {}

fn validate_common(output: Option<&str>, other: &[String]) -> Result<(), Error> {
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
