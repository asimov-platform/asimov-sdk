// This is free and unencumbered software released into the public domain.

//! Streaming HTTP-based API proxy support.
//!
//! [`ProxyConfig`] parses HTTP(S) and SOCKS5 proxy URLs without `std`. The
//! `std` feature adds the `openai` protocol module, optional `BodyLogger`, and
//! explicit environment discovery through `ProxyConfig::from_env`. Transport
//! configuration and body logging are shared across protocol implementations.
//!
//! The caller supplies credentials, options, a bound listener, and a shutdown
//! future. The library does not install signal handlers or select log files.
//! Optional diagnostics use the `tracing` feature.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

mod error;
pub use error::Error;

mod proxy_config;
pub use proxy_config::ProxyConfig;

#[cfg(feature = "std")]
mod body_logger;
#[cfg(feature = "std")]
pub use body_logger::BodyLogger;

#[cfg(feature = "std")]
mod proxy_connector;
#[cfg(feature = "std")]
mod proxy_stream;

#[cfg(feature = "std")]
pub mod openai;

#[cfg(feature = "std")]
type BoxError = alloc::boxed::Box<dyn core::error::Error + Send + Sync>;
