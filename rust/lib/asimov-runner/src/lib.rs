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
//! [`Execute`]. Execution requires a Tokio runtime with process and I/O support.
//!
//! # Input and output
//!
//! [`Input`] selects empty stdin, an asynchronous byte reader, or a JSONL line
//! stream. [`Output`] selects how a child's standard output is handled. Aliases
//! such as [`GraphInput`] and [`TextOutput`] describe a stream's intended content;
//! they do not parse, validate, or convert that content.
//!
//! With `std`, graph producers return a live [JSONL stream][jsonl] of byte-vector
//! lines. Graph consumers accept `GraphInput::Jsonl` for direct composition, or
//! adapt an asynchronous reader into lines. Other results are buffered until completion.
//! Choose [`Output::Captured`] to retrieve stdout. Pattern-specific behavior and
//! current limitations are described in the `programs` module.
//! An empty capture can mean that stdout was discarded, inherited, or replaced
//! by a program-selected output file; it does not establish an empty logical
//! result. Captured output has no configured size limit in this API.
//!
//! # Example
//!
//! Configure a fetcher to capture the graph emitted for a resource:
//!
//! ```no_run
//! # #[cfg(feature = "std")]
//! # async fn example() -> Result<(), asimov_runner::ExecutorError> {
//! use asimov_runner::{Fetcher, FetcherOptions, GraphOutput};
//! use futures_lite::StreamExt;
//!
//! let mut fetcher = Fetcher::new(
//!     "asimov-example-fetcher",
//!     "https://example.com/resource",
//!     GraphOutput::Captured,
//!     FetcherOptions::default(),
//! );
//! let mut graph = fetcher.execute().await?;
//! while let Some(line) = graph.next().await {
//!     let bytes = line?;
//!     // Process this JSONL graph line.
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `std` enables the executor, completion outcomes, execution errors, JSONL transport (including
//!   `Input::Jsonl`), and program wrappers.
//! - `tracing` enables exit-status trace events for buffered and streaming execution.
//! - `all` enables `tracing`; the default features enable both `all` and `std`.
//! - `unstable` is reserved for future use and currently enables no behavior.
//!
//! The crate declares `no_std` and uses `alloc`; input/output types and the
//! [`Pipeline`] placeholder are not gated on `std`. Dependencies may still
//! require the standard library when that feature is disabled.
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
pub use clientele::SysexitsError;
pub use tokio::process::Command;

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

#[cfg(feature = "std")]
pub mod jsonl;
#[cfg(feature = "std")]
pub use jsonl::*;

pub mod output;
pub use output::*;

pub mod pipeline;
pub use pipeline::*;

#[cfg(feature = "std")]
pub mod programs;
#[cfg(feature = "std")]
pub use programs::*;

#[cfg(all(test, feature = "std", unix))]
mod transport_tests;
