// This is free and unencumbered software released into the public domain.

//! Shared HTTP integrity for clients and servers.
//!
//! SHA-256 is the sole digest algorithm (RFC 9530). With `std`, this crate
//! implements RFC 9421 Ed25519 signatures using the protocol crate's keys,
//! HTTP message-component canonicalization, and frame-preserving body adapters.
//! Without `std`, SHA-256 hashing and digest-field encoding remain available.
//!
//! Signature processing operates on an explicit message context, independently
//! of HTTP versions and body types. Servers supply the externally visible target
//! URI, including trusted proxy routing context, rather than guessing a scheme
//! from an incoming origin-form URI. See `Message` and `SignatureInput`.
//!
//! Streaming bodies deliver provisional data and verify at EOF. Callers must
//! consume the entire stream and observe its terminal result before treating
//! the message as authenticated. Dropping a body early never verifies it.
//! Application authorization and atomic nonce-replay rejection follow successful
//! signature AND body verification; freshness checks alone do not prevent replay.
//!
//! # Buffered requests
//!
//! ```no_run
//! # #[cfg(feature = "std")] {
//! use asimov_http::{
//!     SecretKey, SignatureInput, SignatureParams, Signer, VerificationPolicy,
//!     REQUEST_SIGNATURE_TAG, body::sign_request, http::Request,
//! };
//!
//! let key = SecretKey::generate(); // Or load the protocol identity from keyring.
//! let input = SignatureInput::new(
//!     VerificationPolicy::request().required_components,
//!     SignatureParams::fresh(&key.public(), REQUEST_SIGNATURE_TAG)?,
//! )?;
//! let signer = Signer::new(key, "asimov", input)?;
//! let target = "https://example.com/fetch";
//! let mut request = Request::post(target)
//!     .header("authorization", "Bearer token")
//!     .header("accept", "application/jsonl")
//!     .header("content-type", "application/json")
//!     .body(br#"{"urls":["example:"]}"#.as_slice())?;
//! sign_request(&mut request, target, &signer)?;
//! # }
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```
//!
//! # Streaming responses
//!
//! ```no_run
//! # #[cfg(feature = "std")] {
//! use asimov_http::{
//!     Component, FieldSection, SecretKey, SignatureInput, SignatureParams,
//!     Signer, body::SigningBody, http::Response,
//! };
//! use bytes::Bytes;
//! use http_body_util::Full;
//!
//! let key = SecretKey::generate();
//! let input = SignatureInput::new(vec![
//!     Component::new("@status")?,
//!     Component::new("content-type")?,
//!     Component::trailer("content-digest")?,
//! ], SignatureParams::fresh(&key.public(), "example-response")?)?;
//! let signer = Signer::new(key, "response", input)?;
//! // Any http_body::Body<Data = Bytes> works, including an unbounded stream.
//! let response = Response::builder()
//!     .header("content-type", "application/jsonl")
//!     .body(Full::new(Bytes::from_static(b"{}\n")))?;
//! let response = SigningBody::response(
//!     response, None, Some(signer), FieldSection::Headers,
//! )?;
//! // Send with Hyper. Content-Digest and Signature are produced at EOF.
//! # }
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod digest;
pub use digest::{ContentDigest, DigestOptions, DigestPreference};

#[cfg(feature = "std")]
mod error;
#[cfg(feature = "std")]
pub use error::Error;

#[cfg(feature = "std")]
mod fields;
#[cfg(feature = "std")]
mod message;
#[cfg(feature = "std")]
pub use message::{Component, FieldSection, Message, StructuredFieldType};

#[cfg(feature = "std")]
mod signature;
#[cfg(feature = "std")]
pub use signature::{
    REQUEST_SIGNATURE_TAG, SignatureFields, SignatureInput, SignatureParams, Signer,
    VerificationPolicy, VerifiedSignature, Verifier, key_id,
};

#[cfg(feature = "std")]
pub use asimov_protocol::{EndpointId as VerifyingKey, SecretKey};
#[cfg(feature = "std")]
pub use http;

#[cfg(feature = "std")]
pub mod body;
