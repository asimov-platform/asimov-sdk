// This is free and unencumbered software released into the public domain.

use crate::ModuleManifest;
use alloc::{string::String, vec::Vec};
use asimov_http::proxy::{OriginProxy, ProxyOptions};
use core::time::Duration;
use thiserror::Error;

/// The URL of the public index of ASIMOV modules, in JSONL format.
pub const INDEX_URL: &str =
    "https://raw.githubusercontent.com/asimov-modules/asimov-modules/master/index.jsonl";

/// A snapshot of the index of publicly available ASIMOV modules.
#[derive(Clone, Debug, Default)]
pub struct Index {
    modules: Vec<ModuleManifest>,
}

impl Index {
    /// Fetches the index from [`INDEX_URL`].
    ///
    /// Uses shared environment/native proxy discovery from `asimov-http`, with
    /// a 10-second connection timeout and a 30-second read timeout. Redirects
    /// are rejected because the proxy policy is scoped to the index's origin.
    ///
    /// # Errors
    ///
    /// Returns proxy discovery, client construction, request, or JSONL parsing
    /// errors. Non-success HTTP statuses, including redirects, return
    /// [`HttpError::NotSuccess`].
    pub async fn fetch() -> Result<Self, FetchIndexError> {
        Self::fetch_url(INDEX_URL).await
    }

    async fn fetch_url(url: &str) -> Result<Self, FetchIndexError> {
        let url = reqwest::Url::parse(url)
            .map_err(|_| asimov_http::proxy::Error::InvalidOrigin("expected an absolute URL"))?;
        let proxy = OriginProxy::resolve(&url, &ProxyOptions::default())?;
        let builder = reqwest::Client::builder()
            .user_agent("asimov-module-registry")
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30));
        let client = proxy.configure_reqwest(builder)?.build()?;

        // Use the exact URL whose origin-scoped proxy policy was resolved.
        Self::fetch_from(&client, url.as_str()).await
    }

    /// Fetches an index using a caller-configured HTTP client.
    ///
    /// The caller owns proxy, redirect, TLS, and timeout policy. For shared proxy
    /// routing, resolve [`OriginProxy`] for `url` and apply
    /// [`OriginProxy::configure_reqwest`] before building the client. Reusing an
    /// origin-scoped client requires checking each URL with
    /// [`OriginProxy::check_origin`].
    ///
    /// # Errors
    ///
    /// Returns request or JSONL parsing errors. Non-success statuses returned
    /// by the client, including unfollowed redirects, yield
    /// [`HttpError::NotSuccess`].
    pub async fn fetch_from(
        client: &reqwest::Client,
        url: impl AsRef<str>,
    ) -> Result<Self, FetchIndexError> {
        let response = client
            .get(url.as_ref())
            .send()
            .await
            .inspect_err(|err| tracing::debug!(?err))?;

        if !response.status().is_success() {
            Err(HttpError::NotSuccess(response.status()))?;
        }

        let content = response
            .text()
            .await
            .inspect_err(|err| tracing::debug!(?err))?;

        Ok(content.parse()?)
    }

    pub fn modules(&self) -> &[ModuleManifest] {
        &self.modules
    }

    /// Searches the index for modules matching the given query.
    ///
    /// The query is split into whitespace-separated terms, and a module matches
    /// when every term occurs, case-insensitively, as a substring of any of the
    /// module's name, label, title, summary, links, provided programs, or
    /// handled inputs. An empty query matches every module.
    ///
    /// The matching modules are returned in index order.
    pub fn search(&self, query: impl AsRef<str>) -> impl Iterator<Item = &ModuleManifest> {
        let terms: Vec<String> = query
            .as_ref()
            .split_whitespace()
            .map(|term| term.to_lowercase())
            .collect();

        self.modules.iter().filter(move |module| {
            if terms.is_empty() {
                return true;
            }

            let haystack = searchable_text(module);
            terms.iter().all(|term| haystack.contains(term.as_str()))
        })
    }
}

impl core::str::FromStr for Index {
    type Err = ParseIndexError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // parse an index in JSONL format, one module manifest per line
        let modules = input
            .lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(line_index, line)| {
                serde_json::from_str::<ModuleManifest>(line)
                    .inspect_err(|err| tracing::debug!(?err, ?line))
                    .map_err(|err| ParseIndexError(line_index + 1, err))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { modules })
    }
}

/// The searchable fields of a module manifest, lowercased and newline-separated.
fn searchable_text(module: &ModuleManifest) -> String {
    let handles = &module.handles;

    let fields = [
        Some(module.name.as_str()),
        module.label.as_deref(),
        module.title.as_deref(),
        module.summary.as_deref(),
    ];

    let lists = [
        &module.links,
        &module.provides.programs,
        &handles.url_protocols,
        &handles.url_prefixes,
        &handles.url_patterns,
        &handles.file_extensions,
        &handles.content_types,
    ];

    let mut text = String::new();
    for field in fields.into_iter().flatten() {
        text.push_str(field);
        text.push('\n');
    }
    for item in lists.into_iter().flatten() {
        text.push_str(item);
        text.push('\n');
    }

    text.to_lowercase()
}

#[derive(Debug, Error)]
pub enum FetchIndexError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Parse(#[from] ParseIndexError),
    /// Shared proxy discovery failed or the target origin is unsupported.
    #[error(transparent)]
    Proxy(#[from] asimov_http::proxy::Error),
}

impl From<reqwest::Error> for FetchIndexError {
    fn from(value: reqwest::Error) -> Self {
        FetchIndexError::Http(HttpError::Http(value))
    }
}

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("HTTP status code was not successful: {0}")]
    NotSuccess(reqwest::StatusCode),
}

#[derive(Debug, Error)]
#[error("failed to deserialize module index on line {0}: {1}")]
pub struct ParseIndexError(pub usize, #[source] pub serde_json::Error);

#[cfg(test)]
mod tests;
