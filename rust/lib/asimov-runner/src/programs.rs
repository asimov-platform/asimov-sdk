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
//! | [`Compiler`] | Prompt compiler | Natural-language text on stdin | SPARQL query |
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
//! All thirteen patterns in the specification have wrappers here and
//! corresponding traits and options in `asimov-patterns`.
//!
//! Content types describe the external program's contract. These wrappers do
//! not parse graphs or transcode input based on format options; they pass options
//! to the child as arguments. The `other` options are appended as individual
//! arguments, without shell expansion. Positional identifiers follow them.
//! Optional format fields are emitted only when set, so leaving one unset
//! delegates its default to the child program. The specification generally uses
//! `jsonl` for RDF streams, `text` for prompts and responses, and `auto` for a
//! reader's input format or a writer's output format.
//! The `jsonl` token alone does not establish RDF interoperability: connected
//! programs must agree on a documented [RDF mapping profile][rdf-mapping].
//! The option types' field-level contracts and defaults are documented in
//! [`asimov-patterns`][options].
//!
//! # File operands
//!
//! `options.input` and `options.output` name formats, not filenames. Use
//! `options.other` for supported file operands, keeping additional options
//! before them. For patterns accepting input and output files, a single operand
//! selects input; use `-` followed by the destination to select stdin and an
//! output file. An indexer's final operand is always its required index path.
//! Adapter, compiler, and runner programs accept only an input-file operand.
//!
//! A named input file replaces stdin as the program's payload source. For
//! stream-input wrappers, pair it with [`Input::Ignored`](crate::Input::Ignored)
//! to avoid also copying bytes to stdin. The prompter always writes its stored
//! prompt, so it does not offer that input-stream choice. A named output file
//! replaces stdout as the payload destination; these wrappers do not read the
//! file back into the returned result.
//!
//! # Execution and results
//!
//! All wrappers capture stderr for [`ExecutorError`](crate::ExecutorError)
//! diagnostics on unsuccessful exits. Most return an in-memory cursor over raw
//! stdout bytes, positioned at zero. [`Output::Captured`](crate::Output::Captured)
//! retrieves those bytes; ignored or inherited stdout yields an empty cursor.
//! Those results are buffered in full. [`Lister`] instead returns a live stream
//! of byte-vector lines, retaining line terminators and reporting exit failures
//! at the end of the stream. [`Indexer`] discards stdout and returns `()` on success.
//! Captures have no configured size bound. For a continuous emitter, a completed
//! result is unavailable until the program terminates. Successful stderr is
//! discarded, and invalid UTF-8 diagnostics are omitted from process-failure
//! errors; stdout from failed processes is not retained in those errors.
//!
//! Stream-input wrappers consume the reader from its current position to EOF
//! before collecting output. Repeated execution does not rewind input. See
//! [`Executor::execute_with_input`](crate::Executor::execute_with_input) for the
//! implications when a child needs concurrent input and output.
//! Dropping an in-progress execution future drops its owned child handle and,
//! under the executor's default kill-on-drop policy, requests termination.
//! Cancellation does not report success or roll back external side effects.
//! [`Pipeline`](crate::Pipeline) is a placeholder and supplies no stage execution
//! or completion tracking.
//!
//! # Current limitations
//!
//! - [`Output::AsyncWrite`](crate::Output::AsyncWrite) requests captured output,
//!   but wrappers do not forward bytes to the supplied writer.
//! - [`Prompter`] always captures stdout and decodes it as UTF-8, regardless of
//!   its output argument. Its separate prompt-writing task is not awaited, so
//!   write failures are not propagated through the execution result.
//! - [`Resolver`] checks process success but currently returns an empty list
//!   instead of parsing stdout.
//! - Stream-input execution copies input before draining output pipes; a child
//!   producing enough output before consuming input can deadlock.
//!
//! These gaps matter when assessing the specification's host requirements;
//! implementing the role traits is not itself a conformance guarantee.
//!
//! [patterns]: https://asimov-specs.github.io/program-patterns/
//! [rdf-mapping]: https://asimov-specs.github.io/program-patterns/#rdf-mapping
//! [options]: https://docs.rs/asimov-patterns/latest/asimov_patterns/programs/

mod adapter;
pub use adapter::*;

mod compiler;
pub use compiler::*;

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
