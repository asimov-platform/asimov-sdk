// This is free and unencumbered software released into the public domain.

//! Transport-independent requests and remote ASIMOV execution adapters.
//!
//! # Requests and encodings
//!
//! [`FetcherRequest`] and [`ListerRequest`] describe resources and requested
//! options. They perform no I/O and contain no service address, credentials, or
//! response-stream configuration. Adapters own transport configuration and
//! protocol mapping; configured operations implement the shared [`Execute`] trait.
//!
//! Request values are available without any features. The `serde` feature adds
//! JSON serialization and parsing with `no_std` and `alloc` support. These
//! encodings describe request data, not routing, authentication, or responses.
//!
//! The opt-in `jcs` feature adds canonical JSON strings and UTF-8 bytes using
//! [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785). It enables `serde` and
//! `std`. Canonicalization uses the requests' serialized representation,
//! including option omission and duration formatting. Only the JCS methods
//! restrict lister offsets and limits to `2^53 - 1` to preserve integer precision.
//! Neither encoding feature enables a transport.
//!
//! # Transports
//!
//! - `http` enables the HTTP(S) adapters in the `http` module, including their
//!   endpoint mapping, authentication, option support, and JSONL response streams.
//! - `iroh` exposes a placeholder module for a future Iroh/QUIC adapter. Its
//!   execution API and wire protocol are not implemented or specified here.
//!
//! Each adapter defines its response types, failures, and execution lifecycle.
//! The default feature bundle includes `http`, `iroh`, and `serde`; disable
//! defaults to use only request values or select individual encodings/transports.
//! Local execution belongs to asimov-runner; domain APIs belong to their own
//! crates.

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
