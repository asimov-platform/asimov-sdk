// This is free and unencumbered software released into the public domain.

//! Asynchronous process execution for ASIMOV program patterns.
//!
//! The [Program Patterns Specification][patterns] defines the roles and
//! command-line contracts of ASIMOV programs, such as importing RDF datasets,
//! querying them with SPARQL, or executing code in a language runtime. This crate
//! supplies process-backed wrappers for those roles.
//! The role traits and option values are defined by [`asimov-patterns`][traits];
//! this crate provides their process transport and concrete result types.
//!
//! With the `std` feature enabled, `Executor` provides low-level command
//! configuration, process management, and exit-status handling. The wrappers in
//! `programs` translate pattern-specific options into command-line arguments
//! and implement the corresponding traits from `asimov-patterns`, including
//! [`Execute`]. Execution requires a Tokio runtime with process, I/O, and time support.
//!
//! # Input and output
//!
//! [`Input`] selects empty stdin, an asynchronous byte reader, or a JSONL batch
//! stream. [`Output`] selects how a child's standard output is handled. Aliases
//! such as [`GraphInput`] and [`TextOutput`] describe a stream's intended content;
//! they do not parse, validate, or convert that content.
//!
//! With `std`, graph producers return a live [JSONL stream][jsonl] of `JsonlBatch`
//! values containing immutable [`JsonlLine`] values with owned or shared storage. Graph consumers accept
//! `GraphInput::Jsonl` for direct composition, or adapt byte readers into batches.
//! `BatchOptions` controls count, byte target, and collection delay; `flatten_batches`
//! adapts the result for line-at-a-time consumers. Other results are buffered until completion.
//! Choose [`Output::Captured`] to retrieve stdout. Pattern-specific behavior and
//! current limitations are described in the `programs` module.
//! With `std`, `Pipeline::new(source).pipe(consumer)` constructs a typed linear
//! graph pipeline with native OS pipes and coordinated stage completion.
//! An empty capture can mean that stdout was discarded, inherited, or replaced
//! by a program-selected output file; it does not establish an empty logical
//! result. Captured output has no configured size limit in this API.
//!
//! Import [`Stream`] and [`StreamExt`] from this crate to implement and consume
//! streams. The [`stream` module](mod@stream) supplies constructors such as
//! `iter`, `empty`, and `pending`; [`stream!`](macro@stream) and
//! [`try_stream!`](macro@try_stream) build asynchronous generators. These are
//! re-exports, so callers need no direct `futures-lite` or `async-stream`
//! dependency for these operations.
//! [`Bytes`] and [`BytesMut`] are also re-exported for shared-buffer integration.
//! Line constructors enforce framing; they do not parse JSON or validate UTF-8.
//!
//! # Example
//!
//! Configure a fetcher to capture the graph emitted for a resource:
//!
//! ```no_run
//! # #[cfg(feature = "std")]
//! # async fn example() -> Result<(), asimov_runner::ExecutorError> {
//! use asimov_runner::{Fetcher, FetcherOptions, GraphOutput, StreamExt};
//!
//! let mut fetcher = Fetcher::new(
//!     "asimov-example-fetcher",
//!     "https://example.com/resource",
//!     GraphOutput::Captured,
//!     FetcherOptions::default(),
//! );
//! let mut graph = fetcher.execute().await?;
//! while let Some(batch) = graph.next().await {
//!     for bytes in batch?.lines() {
//!         // Process a JSONL line, or pass the whole batch to a service.
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `std` enables the executor, completion outcomes, execution errors, JSONL transport (including
//!   `Input::Jsonl`), pipelines, and program wrappers.
//! - `tracing` enables exit-status trace events for buffered and streaming execution.
//! - `all` enables `tracing`; the default features enable both `all` and `std`.
//! - `unstable` is reserved for future use and currently enables no behavior.
//!
//! The crate declares `no_std` and uses `alloc`; line types and basic input/output policy
//! types are not gated on `std`. Pipelines and process execution require `std`.
//! Dependencies may still require the standard library when that feature is disabled.
//!
//! [patterns]: https://asimov-specs.github.io/program-patterns/
//! [traits]: https://docs.rs/asimov-patterns
//! [jsonl]: https://docs.rs/asimov-runner/latest/asimov_runner/type.JsonlStream.html

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub use asimov_patterns::Execute;
pub use asimov_patterns::OptionSupport;
pub use async_stream::{stream, try_stream};
pub use bytes::{Bytes, BytesMut};
pub use clientele::SysexitsError;
pub use futures_lite::{Stream, StreamExt, stream};
pub use tokio::process::Command;

#[cfg(feature = "std")]
pub mod batch;
#[cfg(feature = "std")]
pub use batch::*;

#[cfg(feature = "std")]
pub mod command_ext;
#[cfg(feature = "std")]
pub use command_ext::*;

#[cfg(feature = "std")]
pub mod completion;
#[cfg(feature = "std")]
pub use completion::*;

#[cfg(feature = "std")]
pub mod executor;
#[cfg(feature = "std")]
pub use executor::*;

#[cfg(feature = "std")]
pub mod executor_error;
#[cfg(feature = "std")]
pub use executor_error::*;

pub mod input;
pub use input::*;

pub mod line;
pub use line::*;

#[cfg(feature = "std")]
pub mod jsonl;
#[cfg(feature = "std")]
pub use jsonl::*;

pub mod output;
pub use output::*;

#[cfg(feature = "std")]
pub mod pipeline;
#[cfg(feature = "std")]
pub use pipeline::*;

#[cfg(feature = "std")]
pub mod programs;
#[cfg(feature = "std")]
pub use programs::*;

#[cfg(all(test, feature = "std", unix))]
mod transport_tests;
