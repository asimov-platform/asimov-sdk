// This is free and unencumbered software released into the public domain.

//! Remote implementations of ASIMOV component patterns.
//!
//! Configured operations implement [`Execute`]; their live results use the raw
//! JSONL batches defined in asimov-flow. Local execution belongs to asimov-runner.
//! This crate has no dependency on a local process executor or social domain API.
//!
//! [`FetcherRequest`] and [`ListerRequest`] own request bodies independently of
//! an executor. The `serde` feature enables JSON serialization and parsing,
//! including without the `http` or `std` features.
//!
//! The opt-in `jcs` feature adds canonical JSON strings and UTF-8 bytes using
//! [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785). It enables `serde` and
//! `std`. Canonicalization uses the requests' serialized representation,
//! including option omission and duration formatting. Lister offsets and limits
//! must be at most `2^53 - 1` to preserve integer precision.
//!
//! The `http` feature provides rustls-backed HTTP with HTTP/2 negotiation and
//! HTTP/1.1 streaming fallback. The initial fetch/list protocol uses JSON POST
//! requests and `application/jsonl` responses. The `iroh` feature exposes an empty
//! module reserved for the future Iroh/QUIC transport.
//!
//! Successful startup does not mean completed execution. Consume the returned
//! stream to EOF to observe body failures. Dropping it cancels further local
//! consumption, without promising cancellation of work on the remote server.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub use asimov_patterns::{
    CachingOptions, Execute, FetcherOptions, FilteringOptions, ListerOptions, TimingOptions,
};

mod requests;
pub use requests::{FetcherRequest, ListerRequest};

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "iroh")]
pub mod iroh;
