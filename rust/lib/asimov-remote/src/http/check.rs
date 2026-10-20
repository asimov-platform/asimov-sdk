// This is free and unencumbered software released into the public domain.

use super::{Error, Executor, ResponseVerification, StreamExt, TimeoutPhase, rate_limit, timeouts};
use asimov_http::{
    Message, SignatureParams,
    body::{BodyError, VerifyingBody},
};
use core::time::Duration;
use http_body_util::BodyExt as _;
use reqwest::{Method, header};
use secrecy::ExposeSecret as _;
use tokio::time::Instant;

impl Executor {
    /// Checks the service with a signed `OPTIONS` request and verified response.
    ///
    /// Sends an empty body to the original service URL, preserving its path and
    /// query string (fragments are omitted). Requires [`Self::with_signing_key`]
    /// and enabled content digests. Uses the shared request signature profile,
    /// bearer credential, proxy policy, connection pool, and transport deadlines.
    ///
    /// An explicit response verifier is honored; otherwise the remote's signing
    /// key is discovered, even if fetch/list response verification is disabled.
    /// The default check policy requires signed status and a matching SHA-256
    /// content digest, including for empty responses such as HTTP 204. A content
    /// type is not required. Signature freshness is checked at header receipt.
    /// The entire body and any trailers are verified before returning success.
    ///
    /// Returns the elapsed round-trip time from dispatch through verified EOF,
    /// including connection setup if needed, but excluding key discovery and
    /// request signing. A successful HTTP status (2xx) is required. Redirects,
    /// HTTP errors, unsigned/invalid responses, and timeouts are errors; no
    /// redirects or retries are performed. The optional total budget includes
    /// discovery, signing, and complete response verification.
    pub async fn check(&self) -> Result<Duration, Error> {
        let total = timeouts::Deadline::new(self.timeouts.total, TimeoutPhase::Total)?;
        timeouts::until(total, async {
            if self.signing_key.is_none() {
                return Err(
                    asimov_http::Error::Policy("remote checks require a signing key").into(),
                );
            }
            if !self.digests.content {
                return Err(asimov_http::Error::Policy("signing requires a content digest").into());
            }
            let mut request = self
                .client
                .request(Method::OPTIONS, self.service_url.clone())
                .bearer_auth(self.api_token.expose_secret())
                .header(header::ACCEPT, "*/*")
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::CONTENT_LENGTH, "0")
                .header(header::TE, "trailers")
                .header(header::CONNECTION, "te")
                .body("")
                .build()?;
            request.headers_mut().extend(self.digests.headers(b""));
            let verifier = match &self.response_verification {
                ResponseVerification::Explicit(verifier) => verifier.clone(),
                ResponseVerification::Disabled | ResponseVerification::Remote => {
                    self.remote_identity
                        .check_verifier(&self.client, &self.base_url, self.timeouts)
                        .await?
                },
            };
            // Discovery can outlive a signature's validity window.
            self.sign_request(&mut request)?;
            let context = Message::request(
                request.method().clone(),
                request.url().as_str(),
                request.headers().clone(),
            )?;
            let started = Instant::now();
            let response = timeouts::limit(
                self.timeouts.response_headers,
                TimeoutPhase::ResponseHeaders,
                self.client.execute(request),
            )
            .await??;
            let received = SignatureParams::now()?;
            let status = if response.status().is_redirection() {
                Err(Error::Redirect(response.status()))
            } else {
                rate_limit::check(&response).and_then(|()| {
                    response.error_for_status_ref()?;
                    if response.status().is_success() {
                        Ok(())
                    } else {
                        Err(Error::UnexpectedStatus(response.status()))
                    }
                })
            };
            let response: asimov_http::http::Response<reqwest::Body> = response.into();
            let body = VerifyingBody::response(response, Some(context), Some(verifier), received)?
                .into_body();
            let mut chunks = timeouts::TimedStream::new(
                body.into_data_stream().map(|chunk| {
                    chunk.map_err(|error| match error {
                        BodyError::Inner(error) => Error::Http(error),
                        BodyError::Integrity(error) => Error::Integrity(error),
                    })
                }),
                self.timeouts.body_idle,
                total,
            );
            while let Some(chunk) = chunks.next().await {
                chunk?;
            }
            status?;
            Ok(started.elapsed())
        })
        .await?
    }
}
