// This is free and unencumbered software released into the public domain.

//! Frame-preserving HTTP/1.1 and HTTP/2 body adapters.
//!
//! `SigningBody` hashes DATA frames as they are polled and emits a final trailer
//! frame containing the digest and signature. It never buffers the content.
//! `VerifyingBody` preserves data frame boundaries and withholds trailers until
//! EOF verification succeeds. Data delivered before then is provisional.
//!
//! HTTP/1.1 senders must use chunked framing and receivers must advertise
//! `TE: trailers`; HTTP/2 carries trailing HEADERS directly. `SigningBody`
//! removes Content-Length and declares its trailers before transmission.
//! Content coding must happen before signing and decoding after verification.

use crate::{
    ContentDigest, Error, FieldSection, Message, Signer, VerifiedSignature, Verifier, fields,
};
use alloc::{boxed::Box, string::String};
use bytes::Bytes;
use core::{
    pin::Pin,
    task::{Context, Poll},
};
use http::{
    HeaderMap, HeaderValue, Request, Response, Version,
    header::{CONTENT_LENGTH, TRAILER, TRANSFER_ENCODING},
};
use http_body::{Body, Frame, SizeHint};
use sha2::{Digest as _, Sha256};

/// A source-body failure or terminal integrity failure.
#[derive(Debug, thiserror::Error)]
pub enum BodyError<E> {
    /// The underlying transport/body failed; no completion fields are emitted.
    #[error("HTTP body failed: {0}")]
    Inner(E),
    /// Digest/signature construction or verification failed.
    #[error(transparent)]
    Integrity(#[from] Error),
}

/// Signs an already buffered request in its headers, without reserializing it.
/// The signer must cover the SHA-256 content digest in the header section.
pub fn sign_request<B: AsRef<[u8]>>(
    request: &mut Request<B>,
    effective_uri: &str,
    signer: &Signer,
) -> Result<(), Error> {
    if !signer.input().covers_digest(FieldSection::Headers) {
        return Err(Error::Policy("unsigned content digest"));
    }
    let mut message = Message::from_request(request, effective_uri)?;
    message.headers.insert(
        "content-digest",
        fields::header(ContentDigest::new(request.body().as_ref()).to_field_value())?,
    );
    signer.sign(&message)?.append_to(&mut message.headers)?;
    *request.headers_mut() = message.headers;
    Ok(())
}

/// Verifies a buffered request's signature AND body. The caller may then
/// authorize the key and atomically consume the authenticated nonce.
pub fn verify_request<B: AsRef<[u8]>>(
    request: &Request<B>,
    effective_uri: &str,
    verifier: &Verifier,
    now: i64,
) -> Result<VerifiedSignature, Error> {
    let message = Message::from_request(request, effective_uri)?;
    let verified = verifier.verify(&message, now)?;
    require_digest_coverage(&verified, &message)?;
    ContentDigest::new(request.body().as_ref()).verify(&message.headers, &message.trailers)?;
    Ok(verified)
}

/// Signs a buffered response in its headers. Representation data and framing
/// remain unchanged; only a content digest and signature fields are added.
pub fn sign_buffered_response<B: AsRef<[u8]>>(
    response: &mut Response<B>,
    request: Option<Message>,
    signer: &Signer,
) -> Result<(), Error> {
    if !signer.input().covers_digest(FieldSection::Headers) {
        return Err(Error::Policy("unsigned content digest"));
    }
    let mut message = Message::response(response.status(), response.headers().clone(), request)?;
    let content = response.body().as_ref();
    if !message.permits_response_trailers() && !content.is_empty() {
        return Err(Error::InvalidField(
            "content is forbidden for this response",
        ));
    }
    message.headers.insert(
        "content-digest",
        fields::header(ContentDigest::new(content).to_field_value())?,
    );
    signer.sign(&message)?.append_to(&mut message.headers)?;
    *response.headers_mut() = message.headers;
    Ok(())
}

/// A streaming response body that emits SHA-256 and optional Ed25519 trailers.
/// No `Sync` or `Unpin` bound is imposed on the wrapped body.
pub struct SigningBody<B> {
    inner: Pin<Box<B>>,
    hash: Sha256,
    message: Message,
    signer: Option<Signer>,
    input_section: FieldSection,
    trailers: Option<HeaderMap>,
    done: bool,
}

impl<B> SigningBody<B> {
    /// Wraps a response, declaring completion trailers before any data is sent.
    ///
    /// `None` emits only the digest. With a signer, `Signature` is a trailer;
    /// `Signature-Input` can be in either section. The input must cover
    /// `"content-digest";tr` (optionally with `sf` or `key="sha-256"`).
    /// HEAD, bodyless status codes, successful CONNECT, and preexisting content
    /// digests are rejected. For known digests/signatures in headers, use
    /// `sign_buffered_response` or the lower-level `Signer` API instead.
    pub fn response(
        mut response: Response<B>,
        request: Option<Message>,
        signer: Option<Signer>,
        input_section: FieldSection,
    ) -> Result<Response<Self>, Error> {
        if !matches!(response.version(), Version::HTTP_11 | Version::HTTP_2) {
            return Err(Error::TrailersUnavailable);
        }
        let mut message =
            Message::response(response.status(), response.headers().clone(), request)?;
        if !message.permits_response_trailers() {
            return Err(Error::TrailersUnavailable);
        }
        if message.headers.contains_key("content-digest") {
            return Err(Error::InvalidField("content digest already supplied"));
        }
        // An unknown size is essential for HTTP/1.1 to keep polling through the
        // final trailer frame, including for empty content.
        message.headers.remove(CONTENT_LENGTH);
        message.headers.remove(TRANSFER_ENCODING);
        let mut names = alloc::vec!["content-digest"];
        if let Some(signer) = &signer {
            if !signer.input().covers_digest(FieldSection::Trailers) {
                return Err(Error::Policy("unsigned trailer digest"));
            }
            check_label(&message.headers, signer.label())?;
            signer.input().preflight(&message)?;
            names.push("signature");
            match input_section {
                FieldSection::Headers => {
                    message
                        .headers
                        .append("signature-input", signer.input_header()?);
                },
                FieldSection::Trailers => names.push("signature-input"),
            }
        }
        declare_trailers(&mut message.headers, &names)?;
        *response.headers_mut() = message.headers.clone();
        Ok(response.map(|inner| Self {
            inner: Box::pin(inner),
            hash: Sha256::new(),
            message,
            signer,
            input_section,
            trailers: None,
            done: false,
        }))
    }

    fn finish(&mut self) -> Result<HeaderMap, Error> {
        let mut trailers = self.trailers.take().unwrap_or_default();
        if trailers.contains_key("content-digest") {
            return Err(Error::InvalidField("body supplied its own digest"));
        }
        let digest = ContentDigest::from_bytes(self.hash.clone().finalize().into());
        trailers.insert("content-digest", fields::header(digest.to_field_value())?);
        if let Some(signer) = &self.signer {
            check_label(&trailers, signer.label())?;
            self.message.trailers = trailers.clone();
            let signature = signer.sign(&self.message)?;
            if self.input_section == FieldSection::Trailers {
                signature.append_to(&mut trailers)?;
            } else {
                trailers.append("signature", signature.signature);
            }
        }
        Ok(trailers)
    }
}

impl<B: Body<Data = Bytes>> Body for SigningBody<B> {
    type Data = Bytes;
    type Error = BodyError<B::Error>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        loop {
            match this.inner.as_mut().poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Err(error))) => {
                    this.done = true;
                    return Poll::Ready(Some(Err(BodyError::Inner(error))));
                },
                Poll::Ready(Some(Ok(frame))) => {
                    if this.trailers.is_some() {
                        this.done = true;
                        return Poll::Ready(Some(Err(Error::FrameOrder.into())));
                    }
                    match frame.into_data() {
                        Ok(data) => {
                            this.hash.update(&data);
                            return Poll::Ready(Some(Ok(Frame::data(data))));
                        },
                        Err(frame) => match frame.into_trailers() {
                            Ok(trailers) => this.trailers = Some(trailers),
                            Err(_) => {
                                this.done = true;
                                return Poll::Ready(Some(Err(Error::FrameOrder.into())));
                            },
                        },
                    }
                },
                Poll::Ready(None) => {
                    this.done = true;
                    return Poll::Ready(Some(
                        this.finish()
                            .map(Frame::trailers)
                            .map_err(BodyError::Integrity),
                    ));
                },
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.done
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

/// A frame-preserving body that verifies headers/trailers and content at EOF.
///
/// Data frames are provisional until `is_verified()` becomes true. Dropping the
/// body, a source error, missing trailers, or an integrity failure never yields
/// verified completion. A failure is emitted once and the body then terminates.
pub struct VerifyingBody<B> {
    inner: Pin<Box<B>>,
    hash: Sha256,
    message: Message,
    verifier: Option<Verifier>,
    now: i64,
    trailers: Option<HeaderMap>,
    done: bool,
    verified: bool,
    signature: Option<VerifiedSignature>,
}

impl<B> VerifyingBody<B> {
    /// Wraps a body and its immutable message context. `None` verifies only the
    /// SHA-256 digest; a verifier additionally requires authenticated coverage
    /// of that digest. `now` is the caller's chosen validation instant.
    pub fn new(inner: B, mut message: Message, verifier: Option<Verifier>, now: i64) -> Self {
        // Only trailers observed from this body can establish its completion.
        message.trailers.clear();
        Self {
            inner: Box::pin(inner),
            hash: Sha256::new(),
            message,
            verifier,
            now,
            trailers: None,
            done: false,
            verified: false,
            signature: None,
        }
    }

    /// Wraps an HTTP response of any body type without consuming its data.
    pub fn response(
        response: Response<B>,
        request: Option<Message>,
        verifier: Option<Verifier>,
        now: i64,
    ) -> Result<Response<Self>, Error> {
        let message = Message::response(response.status(), response.headers().clone(), request)?;
        Ok(response.map(|inner| Self::new(inner, message, verifier, now)))
    }

    /// True only after the source reached EOF and all required checks succeeded.
    pub const fn is_verified(&self) -> bool {
        self.verified
    }

    /// Authenticated signature metadata, available only after full verification.
    pub fn verified_signature(&self) -> Option<&VerifiedSignature> {
        self.signature.as_ref()
    }

    fn finish(&mut self) -> Result<Option<HeaderMap>, Error> {
        let trailers = self.trailers.take();
        if let Some(trailers) = &trailers {
            self.message.trailers = trailers.clone();
        }
        ContentDigest::from_bytes(self.hash.clone().finalize().into())
            .verify(&self.message.headers, &self.message.trailers)?;
        if let Some(verifier) = &self.verifier {
            let signature = verifier.verify(&self.message, self.now)?;
            require_digest_coverage(&signature, &self.message)?;
            self.signature = Some(signature);
        }
        self.verified = true;
        Ok(trailers)
    }
}

impl<B: Body<Data = Bytes>> Body for VerifyingBody<B> {
    type Data = Bytes;
    type Error = BodyError<B::Error>;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        loop {
            match this.inner.as_mut().poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Some(Err(error))) => {
                    this.done = true;
                    return Poll::Ready(Some(Err(BodyError::Inner(error))));
                },
                Poll::Ready(Some(Ok(frame))) => {
                    if this.trailers.is_some() {
                        this.done = true;
                        return Poll::Ready(Some(Err(Error::FrameOrder.into())));
                    }
                    match frame.into_data() {
                        Ok(data) => {
                            this.hash.update(&data);
                            return Poll::Ready(Some(Ok(Frame::data(data))));
                        },
                        Err(frame) => match frame.into_trailers() {
                            Ok(trailers) => this.trailers = Some(trailers),
                            Err(_) => {
                                this.done = true;
                                return Poll::Ready(Some(Err(Error::FrameOrder.into())));
                            },
                        },
                    }
                },
                Poll::Ready(None) => {
                    this.done = true;
                    return Poll::Ready(match this.finish() {
                        Ok(Some(trailers)) => Some(Ok(Frame::trailers(trailers))),
                        Ok(None) => None,
                        Err(error) => Some(Err(error.into())),
                    });
                },
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.done
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

fn require_digest_coverage(signature: &VerifiedSignature, message: &Message) -> Result<(), Error> {
    for section in [FieldSection::Headers, FieldSection::Trailers] {
        if signature.input.covers_digest(section)
            && let Some(value) = fields::combined(&[message.fields(section)], "content-digest")?
            && ContentDigest::parse(&value).is_ok()
        {
            return Ok(());
        }
    }
    Err(Error::Policy(
        "signature does not cover the verified SHA-256 digest",
    ))
}

fn check_label(headers: &HeaderMap, label: &str) -> Result<(), Error> {
    for name in ["signature-input", "signature"] {
        if let Some(value) = fields::combined(&[headers], name)?
            && fields::dictionary(&value)?.contains_key(label)
        {
            return Err(Error::DuplicateLabel);
        }
    }
    Ok(())
}

fn declare_trailers(headers: &mut HeaderMap, names: &[&str]) -> Result<(), Error> {
    let previous = fields::combined(&[headers], "trailer")?.unwrap_or_default();
    let mut declared: alloc::vec::Vec<String> = previous
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    for name in names {
        if !declared.iter().any(|entry| entry == name) {
            declared.push((*name).into());
        }
    }
    headers.insert(
        TRAILER,
        HeaderValue::try_from(declared.join(", ")).map_err(|_| Error::InvalidField("Trailer"))?,
    );
    Ok(())
}
