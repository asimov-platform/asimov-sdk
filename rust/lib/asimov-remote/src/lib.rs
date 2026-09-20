// This is free and unencumbered software released into the public domain.

//! Remote implementations of ASIMOV component patterns.
//!
//! Configured operations implement [`Execute`]; their live results use the raw
//! JSONL batches defined in asimov-flow. Local execution belongs to asimov-runner.
//! This crate has no dependency on a local process executor or social domain API.
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

pub use asimov_patterns::Execute;

#[cfg(feature = "http")]
pub mod http;

#[cfg(feature = "iroh")]
pub mod iroh;
