// This is free and unencumbered software released into the public domain.

//! Process-backed implementations of the [ASIMOV program patterns][patterns].
//!
//! Each wrapper owns an [`Executor`](crate::Executor), its input/output
//! configuration, and a pattern-specific options value re-exported here from
//! `asimov-patterns`. Constructors prepare commands; each `execute` call starts a
//! new child process. Every wrapper also implements [`Execute`](crate::Execute)
//! and its corresponding pattern trait.
//!
//! # Choosing a program
//!
//! The table covers the wrappers available in this crate. Roles and intended
//! payloads follow the specification; the Rust API transports serialized bytes.
//!
//! | Type | Role | Input supplied to the child | Intended output |
//! | --- | --- | --- | --- |
//! | [`Adapter`] | RDF dataset proxy | SPARQL on stdin | RDF |
//! | [`Emitter`] | Value generator | No stdin | RDF |
//! | [`Fetcher`] | URL protocol client | URL as an argument | RDF |
//! | [`Indexer`] | Persistent RDF dataset indexer | RDF on stdin | No output value |
//! | [`Lister`] | Directory iterator | URL as an argument | RDF |
//! | [`Matcher`] | Exact or approximate matcher | RDF on stdin | RDF describing matches |
//! | [`Prompter`] | LLM inference provider | Formatted [`Prompt`] on stdin | Response text |
//! | [`Reader`] | RDF dataset importer | Arbitrary bytes on stdin | RDF |
//! | [`Reasoner`] | RDF dataset entailer | RDF on stdin | Entailed RDF |
//! | [`Resolver`] | URI resolver | URI as an argument | URLs (not yet parsed) |
//! | [`Runner`] | Language runtime engine | Program text on stdin | Execution result as text |
//! | [`Writer`] | RDF dataset exporter | RDF on stdin | Serialized bytes |
//!
//! Content types describe the external program's contract. These wrappers do
//! not parse graphs or transcode input based on format options; they pass options
//! to the child as arguments. The `other` options are appended as individual
//! arguments, without shell expansion. Positional identifiers follow them.
//! Optional format fields are emitted only when set, so leaving one unset
//! delegates its default to the child program. The specification generally uses
//! `jsonl` for RDF streams, `text` for prompts and responses, and `auto` for a
//! reader's input format or a writer's output format.
//!
//! # Execution and results
//!
//! All wrappers capture stderr for [`ExecutorError`](crate::ExecutorError)
//! diagnostics on unsuccessful exits. Most return an in-memory cursor over raw
//! stdout bytes, positioned at zero. [`Output::Captured`](crate::Output::Captured)
//! retrieves those bytes; ignored or inherited stdout yields an empty cursor.
//! Output is buffered in full, not returned as a live stream. [`Indexer`] instead
//! discards stdout and returns `()` on success.
//!
//! Stream-input wrappers consume the reader from its current position to EOF
//! before collecting output. Repeated execution does not rewind input. See
//! [`Executor::execute_with_input`](crate::Executor::execute_with_input) for the
//! implications when a child needs concurrent input and output.
//!
//! # Current limitations
//!
//! - [`Output::AsyncWrite`](crate::Output::AsyncWrite) requests captured output,
//!   but wrappers do not forward bytes to the supplied writer.
//! - [`Prompter`] always captures stdout and decodes it as UTF-8, regardless of
//!   its output argument.
//! - [`Resolver`] checks process success but currently returns an empty list
//!   instead of parsing stdout.
//!
//! [patterns]: https://asimov-specs.github.io/program-patterns/

mod adapter;
pub use adapter::*;

mod emitter;
pub use emitter::*;

mod fetcher;
pub use fetcher::*;

mod indexer;
pub use indexer::*;

mod lister;
pub use lister::*;

mod matcher;
pub use matcher::*;

mod prompter;
pub use prompter::*;

mod reader;
pub use reader::*;

mod reasoner;
pub use reasoner::*;

mod resolver;
pub use resolver::*;

mod runner;
pub use runner::*;

mod writer;
pub use writer::*;
