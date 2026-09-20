// This is free and unencumbered software released into the public domain.

//! Shared data-flow primitives for local and remote ASIMOV components.
//!
//! JSONL lines and batches preserve serialized bytes rather than parsing graphs.
//! Batch types, fallible chunk framing, and batch flattening are runtime-independent.
//! The `tokio` feature adds asynchronous reader adapters and bounded, latency-aware
//! batching. Stream errors are generic: this crate does not
//! depend on local or remote executors. Batch boundaries are transport groupings,
//! not component ports, logical entries, or graph boundaries.
//!
//! The existing async-flow model and port abstractions are re-exported. General
//! component scheduling and transport-independent pipeline supervision are future
//! layers; local subprocess pipeline supervision lives in asimov-runner today.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub use async_flow::*;
pub use bytes::{Bytes, BytesMut};
pub use futures_lite::{Stream, StreamExt, stream};

pub mod line;
pub use line::*;

pub mod batch;
pub use batch::*;

pub mod jsonl;
pub use jsonl::*;
