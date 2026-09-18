// This is free and unencumbered software released into the public domain.

//! Execution traits and configuration values for ASIMOV program patterns.
//!
//! A *pattern* describes a program's role and logical input/output contract:
//! for example, a [`Reader`] imports a document into RDF, an [`Adapter`] queries
//! a dataset with SPARQL, and a [`Writer`] exports RDF to another representation.
//! The [Program Patterns Specification][pps] defines their command-line
//! interfaces and the requirements for hosts that invoke them.
//!
//! This crate provides two building blocks:
//!
//! - [`Execute<T, E>`] is the asynchronous operation shared by all pattern traits.
//! - [`programs`] contains role-specific marker traits and owned option values
//!   such as [`ReaderOptions`], with builders for configuring invocations.
//!
//! Executable lookup, process management, stream transport, and result decoding
//! belong to implementations. The companion [`asimov-runner`][runner] crate
//! supplies Tokio-backed process wrappers and re-exports these option types.
//! Consult its documentation for supported output modes and current limitations;
//! implementing a marker trait does not by itself establish PPS conformance.
//!
//! # Configuring an operation
//!
//! ```
//! use asimov_patterns::ListerOptions;
//!
//! let options = ListerOptions::builder()
//!     .limit(25)
//!     .output("jsonl")
//!     .build();
//!
//! assert_eq!(options.limit, Some(25));
//! assert_eq!(options.output.as_deref(), Some("jsonl"));
//! assert!(options.other.is_empty());
//! ```
//!
//! Building options neither executes a program nor validates its capabilities.
//! Unset fields remain `None`: `asimov-runner` omits the corresponding flags and
//! lets the program apply its specified defaults. In particular, `input` and
//! `output` fields name **formats**, not files or stream endpoints.
//!
//! # Payloads and representations
//!
//! Pattern traits describe semantics without fixing a Rust representation for
//! the result. Their type parameter `T` can represent serialized bytes, parsed
//! data, a fallible stream, or another documented result representation; it is
//! not the number of RDF statements or logical results. [`Indexer`] fixes its result to `()`
//! because indexing has no payload output.
//! In `asimov-runner`, graph producers return live JSONL line streams: an `Ok`
//! execution result means the child was spawned, and eventual failures are stream
//! items. See [`programs`] for the distinction between streaming and buffered results.
//!
//! The options do not parse, validate, or transcode RDF. The spec's default
//! `jsonl` token requires a shared [RDF mapping profile][rdf-mapping]; it does
//! not imply JSON-LD or a particular JSON object shape. Composing two graph
//! patterns requires agreement on both serialization and profile.
//!
//! # Features
//!
//! The crate declares `no_std` and uses `alloc` for strings, collections, and
//! boxed futures. None of its public APIs is gated on the `std` feature.
//! The default features are `all` and `std`; `std` enables standard-library
//! support in dependencies. `all`, `tracing`, and `unstable` currently enable no
//! additional behavior in this crate. Disabling `std` here does not guarantee
//! that the dependency graph is usable on a target without a standard library.
//! Currently, a standalone `--no-default-features` build fails in the transitive
//! `dogma` dependency because collection traits are enabled without its `alloc`
//! feature; the ungated API should not be read as a working no-std build guarantee.
//!
//! [pps]: https://asimov-specs.github.io/program-patterns/
//! [runner]: https://docs.rs/asimov-runner
//! [rdf-mapping]: https://asimov-specs.github.io/program-patterns/#rdf-mapping

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod execute;
pub use execute::*;

pub mod programs;
pub use programs::*;
