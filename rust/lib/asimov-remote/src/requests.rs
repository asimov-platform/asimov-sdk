// This is free and unencumbered software released into the public domain.

use alloc::{string::String, vec, vec::Vec};
use asimov_patterns::{
    CachingOptions, FetcherOptions, FilteringOptions, ListerOptions, TimingOptions,
};

#[cfg(feature = "jcs")]
mod canonicalization;
#[cfg(feature = "serde")]
mod serialization;

/// An owned JSON POST body for the `fetch` endpoint.
///
/// With `serde`, this uses `{"urls":[URL],"options":{...}}`. The pattern,
/// caching, filtering, and timing options share the nested `options` object;
/// unset fields and an empty `other` array are omitted. An empty options object
/// is also omitted. Durations use human-readable strings, retaining subsecond
/// precision. Explicit zero durations and empty filters are preserved.
///
/// Parsing and serialization do not require an HTTP client or runtime and do not
/// validate endpoint support. The HTTP fetcher accepts only unset or `jsonl`
/// output and an empty `other` array.
///
/// ```
/// # #[cfg(feature = "serde")] {
/// use asimov_remote::FetcherRequest;
///
/// let request: FetcherRequest = serde_json::from_str(
///     r#"{"urls":["https://example.com/profile"],"options":{"jq":"."}}"#,
/// )?;
/// assert_eq!(request.filtering.jq.as_deref(), Some("."));
/// let json = serde_json::to_string(&request)?;
/// assert_eq!(serde_json::from_str::<FetcherRequest>(&json)?, request);
/// # }
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FetcherRequest {
    /// Resource URLs to fetch, in request order.
    pub urls: Vec<String>,
    /// Pattern options flattened into the JSON `options` object.
    pub options: FetcherOptions,
    /// Cache freshness options flattened into the JSON `options` object.
    pub caching: CachingOptions,
    /// Output filters flattened into the JSON `options` object.
    pub filtering: FilteringOptions,
    /// Execution timing options flattened into the JSON `options` object.
    pub timing: TimingOptions,
}

impl FetcherRequest {
    /// Creates a single-resource request. Set [`Self::urls`] for multiple URLs.
    pub fn new(input: impl Into<String>, options: FetcherOptions) -> Self {
        Self {
            urls: vec![input.into()],
            options,
            ..Self::default()
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

/// An owned JSON POST body for the `list` endpoint.
///
/// With `serde`, this uses `{"url":URL,"options":{...}}`. All [`ListerOptions`]
/// fields and the shared caching, filtering, and timing options are flattened
/// into the nested `options` object. Unset fields and an empty `other` array are
/// omitted, as is the whole object when empty. Explicit zero and empty values
/// are preserved. Durations use human-readable strings.
///
/// Sort keys are comma-separated with a `-` prefix for descending order. Sort
/// keys and custom output formats require only [`core::fmt::Display`] for
/// serialization. Parsing requires `SortKeys<T>` and `F` to implement
/// [`core::str::FromStr`] with displayable errors. An empty sort string represents
/// an empty key list. Neither type needs to implement Serde; both default to
/// [`String`].
///
/// ```
/// # #[cfg(feature = "serde")] {
/// use asimov_remote::ListerRequest;
///
/// let request: ListerRequest = serde_json::from_str(
///     r#"{"url":"https://example.com/collection","options":{"limit":0}}"#,
/// )?;
/// assert_eq!(request.options.limit, Some(0));
/// let json = serde_json::to_string(&request)?;
/// assert_eq!(serde_json::from_str::<ListerRequest>(&json)?, request);
/// # }
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListerRequest<T: Clone = String, F = String> {
    /// Collection URL to list.
    pub url: String,
    /// Pattern options flattened into the JSON `options` object.
    pub options: ListerOptions<T, F>,
    /// Cache freshness options flattened into the JSON `options` object.
    pub caching: CachingOptions,
    /// Output filters flattened into the JSON `options` object.
    pub filtering: FilteringOptions,
    /// Execution timing options flattened into the JSON `options` object.
    pub timing: TimingOptions,
}

impl<T: Clone, F> Default for ListerRequest<T, F> {
    fn default() -> Self {
        Self {
            url: String::new(),
            options: ListerOptions::default(),
            caching: CachingOptions::default(),
            filtering: FilteringOptions::default(),
            timing: TimingOptions::default(),
        }
    }
}

impl<T: Clone, F> ListerRequest<T, F> {
    /// Creates a collection request with typed options.
    pub fn new(input: impl Into<String>, options: ListerOptions<T, F>) -> Self {
        Self {
            url: input.into(),
            options,
            ..Self::default()
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
