// This is free and unencumbered software released into the public domain.

#![cfg(feature = "client")]

use crate::{FetchRequest, ListRequest};
use alloc::string::String;
use asimov_remote::{
    Execute,
    http::{BatchOptions, Executor, Fetcher, Lister},
};
use core::fmt;

pub use asimov_remote::http::{Error as SocialClientError, JsonlStream as SocialResponseStream};

/// A domain-specific façade over ASIMOV's remote HTTP transport.
/// Responses contain raw JSONL batches, preserving bytes and line endings.
///
/// ```no_run
/// use asimov_social::{FetchRequest, SocialClient, SocialClientError};
/// use asimov_remote::http::StreamExt;
///
/// # async fn example() -> Result<(), SocialClientError> {
/// let client = SocialClient::new("api-token")?;
/// let mut batches = client.fetch(FetchRequest {
///     urls: vec!["https://example.com/profile".into()],
///     ..Default::default()
/// }).await?;
/// while let Some(batch) = batches.next().await {
///     for line in batch?.lines() {
///         // Forward raw bytes, or decode them at the application boundary.
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct SocialClient {
    executor: Executor,
}

impl SocialClient {
    /// Creates a rustls-backed client for <https://asimov.social>.
    /// The API token is sent as bearer authentication on every request.
    pub fn new(api_token: impl Into<String>) -> Result<Self, SocialClientError> {
        Self::with_base_url(api_token, "https://asimov.social")
    }

    /// Uses a custom base URL, including an optional path prefix. HTTP/2 is
    /// negotiated with HTTP/1.1 fallback; plain HTTP is supported for development.
    pub fn with_base_url(
        api_token: impl Into<String>,
        base_url: impl AsRef<str>,
    ) -> Result<Self, SocialClientError> {
        Ok(Self {
            executor: Executor::new(base_url, Some(&api_token.into()))?,
        })
    }

    /// Sets the count, byte target, and maximum collection delay for response batches.
    #[must_use]
    pub fn with_batching(mut self, options: BatchOptions) -> Self {
        self.executor = self.executor.with_batching(options);
        self
    }

    /// POSTs to `fetch` under the configured base URL. Supports the service's
    /// multi-URL request in addition to the single-URL fetcher pattern.
    /// Uses the remote HTTP fetcher, which rejects non-JSONL output formats and
    /// additional `other` arguments before sending.
    /// Startup/status errors are returned directly; later body errors are stream
    /// items. Consume the stream to EOF to observe completion.
    pub async fn fetch(
        &self,
        request: FetchRequest,
    ) -> Result<SocialResponseStream, SocialClientError> {
        Fetcher::new(self.executor.clone(), request).execute().await
    }

    /// POSTs to `list` under the configured base URL. Returns raw JSONL batches;
    /// JSON, UTF-8, and the RDF mapping are not validated by this transport.
    /// All request options are forwarded to the endpoint, including zero limits.
    /// Sort keys and custom output formats need only implement [`fmt::Display`]
    /// and the sendable execution bounds. Use `ListRequest::<String>::new` or a
    /// `ListRequest` type annotation when the options do not determine the types.
    pub async fn list<T, F>(
        &self,
        request: ListRequest<T, F>,
    ) -> Result<SocialResponseStream, SocialClientError>
    where
        T: Clone + fmt::Display + Send + Sync,
        F: fmt::Display + Send + Sync,
    {
        Lister::new(self.executor.clone(), request).execute().await
    }
}
