// This is free and unencumbered software released into the public domain.

//! Asynchronous process execution for ASIMOV program patterns.
//!
//! The [Program Patterns Specification][patterns] defines the roles and
//! command-line contracts of ASIMOV programs, such as importing RDF datasets,
//! querying them with SPARQL, or executing code in a language runtime. This crate
//! supplies process-backed wrappers for those roles.
//!
//! With the `std` feature enabled, `Executor` provides low-level command
//! configuration, process management, and exit-status handling. The wrappers in
//! `programs` translate pattern-specific options into command-line arguments
//! and implement the corresponding traits from `asimov-patterns`, including
//! [`Execute`]. Execution requires a Tokio runtime with process and I/O support.
//!
//! # Input and output
//!
//! [`Input`] selects either an empty standard input or an asynchronous byte
//! source. [`Output`] selects how a child's standard output is handled. Aliases
//! such as [`GraphInput`] and [`TextOutput`] describe a stream's intended content;
//! they do not parse, validate, or convert that content.
//!
//! Most program wrappers return captured stdout as an in-memory byte cursor
//! after the process exits successfully. Choose [`Output::Captured`] to retrieve
//! those bytes. Pattern-specific exceptions and current limitations are
//! described in the `programs` module.
//!
//! # Example
//!
//! Configure a fetcher to capture the graph emitted for a resource:
//!
//! ```no_run
//! # #[cfg(feature = "std")]
//! # async fn example() -> Result<(), asimov_runner::ExecutorError> {
//! use asimov_runner::{Fetcher, FetcherOptions, GraphOutput};
//!
//! let mut fetcher = Fetcher::new(
//!     "asimov-example-fetcher",
//!     "https://example.com/resource",
//!     GraphOutput::Captured,
//!     FetcherOptions::default(),
//! );
//! let graph_bytes = fetcher.execute().await?.into_inner();
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `std` enables the executor, execution errors, and program wrappers.
//! - `tracing` enables trace events for child-process exit statuses.
//! - `all` enables `tracing`; the default features enable both `all` and `std`.
//! - `unstable` is reserved for future use and currently enables no behavior.
//!
//! The crate declares `no_std` and uses `alloc`; input/output types and the
//! [`Pipeline`] placeholder are not gated on `std`. Dependencies may still
//! require the standard library when that feature is disabled.
//!
//! [patterns]: https://asimov-specs.github.io/program-patterns/

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub use asimov_patterns::Execute;
pub use clientele::SysexitsError;
pub use tokio::process::Command;

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

pub mod output;
pub use output::*;

pub mod pipeline;
pub use pipeline::*;

#[cfg(feature = "std")]
pub mod programs;
#[cfg(feature = "std")]
pub use programs::*;
