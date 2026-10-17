// This is free and unencumbered software released into the public domain.

//! HTTP(S) execution adapters for resource fetches and collection listings.
//!
//! [`Executor`] owns the service URL, bearer credentials, connection pool, and
//! local response batching. HTTPS uses rustls, with HTTP/2 negotiation and
//! HTTP/1.1 streaming fallback. Plain HTTP is also accepted for development.
//!
//! # HTTP request mapping
//!
//! [`Fetcher`] POSTs a [`FetcherRequest`] to the service's `fetch` endpoint.
//! [`Lister`] POSTs a [`ListerRequest`] to `list`, forwarding every configured
//! [`ListerOptions`] field. Request URLs identify input resources, whereas the
//! executor's base URL selects the remote service.
//!
//! Both adapters send the request's Serde representation as `application/json`
//! and advertise `Accept: application/jsonl`. They use ordinary JSON
//! serialization; enabling `jcs` does not change the HTTP body encoding.
//! The request types document their JSON representation and omission rules.
//!
//! This HTTP protocol delegates cache policy, filters (Jev before jq), and
//! relative execution deadlines to the endpoint. [`Fetcher`] applies additional
//! protocol-specific option restrictions before sending. Endpoint validation
//! determines support for forwarded options. These are HTTP adapter contracts,
//! not restrictions on the transport-independent request types.
//!
//! # Request integrity
//!
//! Every request carries a SHA-256 `Content-Digest` by default, following
//! [RFC 9530](https://www.rfc-editor.org/rfc/rfc9530). [`DigestOptions`] configures
//! `Content-Digest`, optional `Repr-Digest`, and optional `Want-Content-Digest` /
//! `Want-Repr-Digest` preferences, all using SHA-256 exclusively. JSON is
//! serialized once; digests cover those exact UTF-8 bytes, without HTTP framing.
//! The complete JSON body is also the request's selected representation.
//!
//! [`Executor::with_signing_key`] enables RFC 9421 Ed25519 signatures using an
//! `asimov_protocol::SecretKey`. Each execution uses a fresh nonce and a
//! 60-second validity window. The shared asimov-http request profile covers the
//! method, target URI, credentials, and semantic headers. Digests alone do not
//! authenticate messages. Redirects return [`Error::Redirect`]; configure the
//! final service URL.
//!
//! [`Executor::with_response_verification`] discovers the remote public key
//! through the executor's connection pool and requires signed responses.
//! [`Executor::remote_public_key`] exposes the same cached identity to callers.
//! [`Executor::with_response_verifier`] supplies an explicit key and policy.
//! Verification preserves frames, including trailers. JSONL batches are provisional
//! until EOF; an integrity failure is a terminal stream error. Validation time
//! is response-header receipt. Content coding is not automatically decoded.
//!
//! # HTTP responses and lifecycle
//!
//! Both operations implement [`asimov_patterns::Execute`] with [`JsonlStream`]
//! results: raw JSONL batches supplied by asimov-flow with HTTP-specific errors.
//! Request/status failures are returned at startup; subsequent response-body
//! failures are stream items, following any complete buffered records. Consume
//! the stream to EOF to observe these failures. Transport completion does not
//! establish an application-level success outcome. Dropping the stream releases
//! the response, without promising cancellation of work on the remote server.
//!
//! The endpoint must supply the RDF mapping profile expected by its graph
//! consumers; this adapter's JSONL framing does not validate that mapping.
//!
//! # Example
//!
//! ```no_run
//! use asimov_remote::{CachingOptions, Execute, FetcherRequest};
//! use asimov_remote::http::{Executor, Fetcher, Error, StreamExt};
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

use alloc::{boxed::Box, format, string::String, sync::Arc};
use asimov_flow::{BatchStream, jsonl_batches_from_chunks};
use asimov_http::{
    Component, Message, REQUEST_SIGNATURE_TAG, SignatureInput, SignatureParams, Signer,
    VerificationPolicy,
};
use asimov_id::PublicKeyEncoding;
use asimov_keyring::Keyring;
use async_trait::async_trait;
use core::fmt;
use reqwest::{
    Client, Request, Url,
    header::{ACCEPT, CONTENT_LENGTH, HeaderValue},
};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

pub use asimov_http::{DigestOptions, DigestPreference, SecretKey, Verifier};
pub use asimov_id::PublicKey;

mod remote_identity;
use remote_identity::RemoteIdentityCache;

/// Transport-independent request types, re-exported for convenience.
pub use crate::{FetcherRequest, ListerRequest};
/// Shared JSONL framing and batching types used by this HTTP adapter.
pub use asimov_flow::{BatchOptions, JsonlBatch, JsonlLine, StreamExt};
/// Transport-independent pattern options and capability descriptions.
pub use asimov_patterns::{
    CachingOptions, FetcherOptions, FilteringOptions, ListerCapabilities, ListerOptions,
    OptionSupport, OutputFormat, TimingOptions,
};

/// A raw JSONL batch stream with HTTP-specific failures.
pub type JsonlStream = BatchStream<Error>;

/// HTTP adapter configuration, startup, or response-body failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// HTTP request construction, connection, status, or response-body failure.
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    /// Failure to load the local identity used for the HTTP bearer credential.
    #[error("failed to load local public key: {0}")]
    Keyring(#[from] asimov_keyring::KeyringError),
    /// Invalid HTTP service URL or endpoint resolution.
    #[error("invalid base URL: {0}")]
    InvalidBaseUrl(String),
    /// An option rejected by this HTTP protocol adapter.
    #[error("the HTTP fetch/list protocol does not support option {0}")]
    UnsupportedOption(&'static str),
    /// A redirection response, which is never followed automatically.
    #[error("HTTP redirect ({0}); configure the final service URL")]
    Redirect(reqwest::StatusCode),
    /// HTTP digest/signature construction or terminal verification failure.
    #[error(transparent)]
    Integrity(#[from] asimov_http::Error),
    /// Remote identity discovery or revalidation failed.
    #[error("failed to discover remote public key: {0}")]
    RemoteKey(#[source] Arc<dyn core::error::Error + Send + Sync>),
}

#[derive(Clone, Default)]
enum ResponseVerification {
    #[default]
    Disabled,
    Remote,
    Explicit(Verifier),
}

impl ResponseVerification {
    fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

/// Reusable HTTP(S) service configuration.
/// Clones share the connection pool and remote identity cache.
///
/// Owns service addressing, bearer authentication, request digests, and response
/// batching; resource URLs and operation options belong to the request values.
#[derive(Clone)]
pub struct Executor {
    client: Client,
    base_url: Url,
    api_token: SecretString,
    batching: BatchOptions,
    digests: DigestOptions,
    signing_key: Option<SecretKey>,
    response_verification: ResponseVerification,
    remote_identity: Arc<RemoteIdentityCache>,
}

impl fmt::Debug for Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpExecutor")
            .field("base_url", &self.base_url)
            .field("batching", &self.batching)
            .field("digests", &self.digests)
            .field("signing", &self.signing_key.is_some())
            .field(
                "response_verification",
                &self.response_verification.is_enabled(),
            )
            .finish_non_exhaustive()
    }
}

impl Executor {
    /// Configures access to an HTTP(S) service independently of resource URLs.
    ///
    /// Uses rustls with HTTP/2 negotiation and HTTP/1.1 fallback. Plain HTTP URLs
    /// are accepted for development. Path prefixes and an optional trailing slash
    /// are supported; query strings and fragments on the base URL are ignored.
    /// Sends an `Authorization: Bearer` header on operation requests. `Some(token)`
    /// uses the supplied token; `None` loads the current user's public key with
    /// [`Keyring::my_public_key`] and encodes it as Base58, without the non-ASCII
    /// ASIMOV prefix. The token is stored in a [`SecretString`], which zeroizes
    /// its storage on drop.
    ///
    /// The `User-Agent` is the caller name reported by `agent-detector` at
    /// construction, or `asimov-remote/<version>` if detection yields no valid
    /// HTTP header value.
    ///
    /// Requests use [`DigestOptions::default()`]. Redirects are not followed;
    /// configure the service's final URL so request integrity fields remain
    /// bound to the prepared request's method, target, and body.
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
                .redirect(reqwest::redirect::Policy::none())
                // Digests cover content-coded bytes. Disable decoding even if
                // another workspace dependency enables reqwest compression.
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .no_zstd()
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
            digests: DigestOptions::default(),
            signing_key: None,
            response_verification: ResponseVerification::Disabled,
            remote_identity: Arc::default(),
        })
    }

    /// Returns the HTTP service base URL used for endpoint resolution.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Returns the remote service's validated Ed25519 public key.
    ///
    /// Discovery uses this executor's HTTP connection pool, without bearer or
    /// URL credentials. Clones share cached identities and in-flight lookups.
    /// Freshness follows the remote's max-age, Age, no-cache, and no-store
    /// directives, defaulting to five minutes without an explicit policy.
    /// Expired identities are revalidated using an ETag when available.
    ///
    /// This discovers the remote identity independently of an explicitly
    /// configured response verifier. It does not enable response verification.
    ///
    /// # Errors
    ///
    /// Returns [`Error::RemoteKey`] if discovery fails, redirects, exceeds its
    /// 30-second or 64-KiB limit, or does not identify exactly one eligible key.
    pub async fn remote_public_key(&self) -> Result<PublicKey, Error> {
        self.remote_identity
            .public_key(&self.client, &self.base_url)
            .await
    }

    /// Sets local response-stream batching for this HTTP client.
    #[must_use]
    pub fn with_batching(mut self, options: BatchOptions) -> Self {
        self.batching = options;
        self
    }

    /// Configures RFC 9530 digest and preference fields for subsequent requests.
    #[must_use]
    pub fn with_digests(mut self, options: DigestOptions) -> Self {
        self.digests = options;
        self
    }

    /// Enables Ed25519 signing with an explicitly supplied protocol identity.
    /// Each request receives fresh signature parameters. SHA-256 content digests
    /// must remain enabled. This does not change the configured bearer token.
    #[must_use]
    pub fn with_signing_key(mut self, key: SecretKey) -> Self {
        self.signing_key = Some(key);
        self
    }

    /// Requires signed responses using the automatically discovered remote key.
    ///
    /// Discovery is lazy, cached across clones, and uses the operation's HTTP
    /// connection pool. The `response` signature must cover status, content
    /// type, and a matching SHA-256 content digest in headers or trailers.
    /// Freshness and nonce checks use the default HTTP verification policy.
    /// Output remains provisional until EOF verification succeeds.
    ///
    /// Replaces any explicitly configured verifier. Discovery failures prevent
    /// the operation from being sent; an expired key is never used as fallback.
    #[must_use]
    pub fn with_response_verification(mut self) -> Self {
        self.response_verification = ResponseVerification::Remote;
        self
    }

    /// Requires response signatures from the verifier's key and profile, and a
    /// matching SHA-256 content digest in headers or trailers. Batches delivered
    /// before EOF are provisional. Consume to EOF to observe terminal failures.
    ///
    /// Replaces automatic remote-key verification, if enabled.
    /// To let the remote choose digest placement per response, omit the response
    /// digest from [`VerificationPolicy::required_components`]. Requiring
    /// `"content-digest"` pins it to headers; `"content-digest";tr` pins it to
    /// trailers. The body verifier independently requires a matching SHA-256
    /// digest and a valid signature covering it in whichever section contains it.
    ///
    /// # Example
    ///
    /// Configure a trusted remote key and the remote's `response` signature
    /// label, allowing either header or trailer digests:
    ///
    /// ```no_run
    /// # extern crate alloc;
    /// use alloc::vec;
    /// use asimov_http::{
    ///     Component, VerificationPolicy, Verifier, VerifyingKey, key_id,
    /// };
    /// use asimov_remote::http::{Error, Executor};
    ///
    /// fn client(remote_key: VerifyingKey) -> Result<Executor, Error> {
    ///     let policy = VerificationPolicy::new(vec![
    ///         Component::new("@status")?,
    ///         Component::new("content-type")?,
    ///     ]);
    ///     let verifier = Verifier::new(
    ///         remote_key, &key_id(&remote_key), "response", policy,
    ///     )?;
    ///     Ok(Executor::new("https://asimov.social", Some("api-token"))?
    ///         .with_response_verifier(verifier))
    /// }
    /// ```
    #[must_use]
    pub fn with_response_verifier(mut self, verifier: Verifier) -> Self {
        self.response_verification = ResponseVerification::Explicit(verifier);
        self
    }

    /// Serializes a value as JSON and POSTs it to a relative HTTP endpoint.
    ///
    /// Resolves the endpoint under this executor's service base URL and applies
    /// its bearer authentication, digest policy, and local response batching.
    /// The body uses ordinary Serde JSON serialization, independently of `jcs`.
    /// Response bytes are framed as JSONL without JSON or UTF-8 validation.
    /// The stream owns the response, not a borrow of this executor.
    ///
    /// # Errors
    ///
    /// Endpoint resolution, request serialization, connection, redirects, and
    /// HTTP status errors are returned directly. Body errors terminate the
    /// returned stream after any complete buffered records.
    pub async fn post_jsonl(
        &self,
        endpoint: &str,
        request: &impl Serialize,
    ) -> Result<JsonlStream, Error> {
        let mut request = self.prepare_json_request(endpoint, request)?;
        let verifier = match &self.response_verification {
            ResponseVerification::Disabled => None,
            ResponseVerification::Explicit(verifier) => Some(verifier.clone()),
            ResponseVerification::Remote => Some(
                self.remote_identity
                    .verifier(&self.client, &self.base_url)
                    .await?,
            ),
        };
        // Discovery can involve network I/O. Sign only once ready to dispatch.
        self.sign_request(&mut request)?;
        let request_context = if verifier.is_some() {
            Some(Message::request(
                request.method().clone(),
                request.url().as_str(),
                request.headers().clone(),
            )?)
        } else {
            None
        };
        let response = self.client.execute(request).await?;
        if response.status().is_redirection() {
            return Err(Error::Redirect(response.status()));
        }
        let response = response.error_for_status()?;
        if let Some(verifier) = verifier {
            use asimov_http::body::{BodyError, VerifyingBody};
            use http_body_util::BodyExt as _;
            let response: asimov_http::http::Response<reqwest::Body> = response.into();
            let response = VerifyingBody::response(
                response,
                request_context,
                Some(verifier),
                SignatureParams::now()?,
            )?;
            return Ok(jsonl_batches_from_chunks(
                response.into_body().into_data_stream().map(|chunk| {
                    chunk.map_err(|error| match error {
                        BodyError::Inner(error) => Error::Http(error),
                        BodyError::Integrity(error) => Error::Integrity(error),
                    })
                }),
                self.batching,
            ));
        }
        Ok(jsonl_batches_from_chunks(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(Error::from)),
            self.batching,
        ))
    }

    /// Serializes and validates the request before discovery performs network I/O.
    fn prepare_json_request(
        &self,
        endpoint: &str,
        request: &impl Serialize,
    ) -> Result<Request, Error> {
        if self.signing_key.is_some() && !self.digests.content {
            return Err(asimov_http::Error::Policy("signing requires a content digest").into());
        }
        // Endpoints are relative paths so the configured bearer token is always
        // sent to the configured service, including when a caller supplies a path.
        let mut url = self
            .base_url
            .join(endpoint)
            .map_err(|error| Error::InvalidBaseUrl(format!("{error}")))?;
        if url.origin() != self.base_url.origin() {
            return Err(Error::InvalidBaseUrl(
                "endpoint must use the configured origin".into(),
            ));
        }
        // Fragments are not part of the HTTP target URI (RFC 9421, section 2.2.2).
        url.set_fragment(None);
        let mut request = self
            .client
            .post(url)
            .header(ACCEPT, "application/jsonl")
            .bearer_auth(self.api_token.expose_secret())
            .json(request)
            .build()?;
        // RequestBuilder::json serializes once into a buffered body. Hash that
        // body rather than reserializing a possibly stateful Serialize value.
        let body = request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .expect("a successfully built JSON request has a buffered body");
        let length = HeaderValue::from(body.len());
        let digests = self.digests.headers(body);
        request.headers_mut().insert(CONTENT_LENGTH, length);
        request.headers_mut().extend(digests);
        if self.response_verification.is_enabled() {
            request
                .headers_mut()
                .insert(reqwest::header::TE, HeaderValue::from_static("trailers"));
            request
                .headers_mut()
                .insert(reqwest::header::CONNECTION, HeaderValue::from_static("te"));
        }
        Ok(request)
    }

    fn sign_request(&self, request: &mut Request) -> Result<(), Error> {
        if let Some(key) = &self.signing_key {
            let mut components = VerificationPolicy::request().required_components;
            for name in [
                "content-encoding",
                "repr-digest",
                "want-content-digest",
                "want-repr-digest",
            ] {
                if request.headers().contains_key(name) {
                    components.push(Component::new(name)?);
                }
            }
            let input = SignatureInput::new(
                components,
                SignatureParams::fresh(&key.public(), REQUEST_SIGNATURE_TAG)?,
            )?;
            let signer = Signer::new(key.clone(), "asimov", input)?;
            let message = Message::request(
                request.method().clone(),
                request.url().as_str(),
                request.headers().clone(),
            )?;
            signer.sign(&message)?.append_to(request.headers_mut())?;
        }
        Ok(())
    }
}

/// A resource fetch configured for HTTP(S) execution.
///
/// Each execution serializes its [`FetcherRequest`] and POSTs it to the
/// service's `fetch` endpoint, returning a [`JsonlStream`].
///
/// The endpoint applies caching, filtering (Jev before jq), and relative
/// execution deadlines. This HTTP adapter supports only unset or `jsonl` output
/// and an empty `other` array; it rejects other values before sending.
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

/// A collection listing configured for HTTP(S) execution.
///
/// Each execution serializes its [`ListerRequest`] and POSTs it to the
/// service's `list` endpoint, returning a [`JsonlStream`]. Offset and limit are
/// native entry counts, delegated to the endpoint. This HTTP adapter does not
/// equate JSONL lines with entries or impose a separate line cap. Every execution
/// sends a fresh request, including when the limit is zero.
///
/// [`ListerRequest`] defines the request data and its JSON encoding. The HTTP
/// endpoint validates and applies options, including caching, filtering (Jev
/// before jq), and relative execution deadlines.
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

    /// Options this HTTP adapter can forward to the remote service.
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
