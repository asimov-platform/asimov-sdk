// This is free and unencumbered software released into the public domain.

//! Process-backed implementations of the [ASIMOV program patterns][patterns].
//!
//! Each wrapper owns an [`Executor`](crate::Executor), its input/output
//! configuration, and a pattern-specific options value re-exported here from
//! `asimov-patterns`. Constructors prepare commands; execution validates any
//! supplied capabilities before starting a child. A zero-limit [`Lister`]
//! returns immediately after validation without spawning.
//! Every wrapper also implements [`Execute`](crate::Execute)
//! and its corresponding pattern trait.
//! All wrappers set `Execute::Error` to [`ExecutorError`](crate::ExecutorError).
//! Generic bounds use associated-type equality, for example
//! `asimov_patterns::Fetcher<JsonlStream, Error = ExecutorError>` or
//! `asimov_patterns::Indexer<Error = ExecutorError>`.
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
//! | [`Resolver`] | URI resolver | URI as an argument | Parsed absolute URLs |
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
//! Command construction uses [`CommandExt::option`](crate::CommandExt::option)
//! for optional `--name=value` arguments, preserving literal values and order
//! without building temporary argument vectors. Capability-based omission is
//! expressed by filtering the optional value before passing it to the helper.
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
//! Graph producers ([`Adapter`], [`Emitter`], [`Fetcher`], [`Lister`], [`Matcher`],
//! [`Reader`], and [`Reasoner`]) return a live [`JsonlStream`](crate::JsonlStream).
//! Execution returns after spawning; polling yields byte-vector lines, preserving
//! LF/CRLF terminators and an unterminated final line. Spawn errors are returned
//! directly; input, read, and exit errors are stream items. Consume the stream to
//! completion to check process success. Ignored or inherited stdout yields no
//! lines but still checks the exit status when polled to completion.
//! [`Lister`] enforces its configured limit locally as a stdout line cap in every
//! output mode. Native `--limit` support is optional: the flag is forwarded for
//! unknown/supported capability and omitted when explicitly unsupported. The
//! local cap always applies, including protection against buggy subprograms.
//! `--sort`, `--offset`, `--before`, and `--after` are also optional native
//! capabilities. Supply support using [`Lister::with_capabilities`] and
//! [`ListerCapabilities`]. Unknown
//! and supported requests are forwarded; explicitly unsupported typed requests
//! fail before spawning with [`ExecutorError::UnsupportedOption`](crate::ExecutorError::UnsupportedOption).
//! There is no automatic discovery or emulation of sorting, offset, or cursor
//! bounds. Numeric offset and URI cursors are alternative pagination modes;
//! before/after bounds are exclusive in the chosen sort order, using entry
//! JSON-LD `@id` URIs. Limit applies after sorting and pagination. On reaching
//! the line cap it stops the child
//! and ends the stream without checking eventual exit status. A zero limit does
//! not spawn a child. This cap counts serialized lines, not logical RDF entries.
//!
//! Graph consumers ([`Matcher`], [`Reasoner`], [`Indexer`], and [`Writer`]) accept
//! [`GraphInput::Jsonl`](crate::GraphInput::Jsonl) for direct stream composition.
//! Byte readers are adapted into lines. Each input item is written with an LF
//! appended if missing, preserving existing LF/CRLF endings. Blank lines are
//! preserved; JSON, UTF-8, and RDF are not validated. Use `jsonl` (the pattern
//! default) for graph format options; selecting another format does not change
//! the line-based transport.
//!
//! Input feeding, stdout reading, and stderr draining run concurrently with
//! backpressure. Graph-output execution transfers input ownership into the
//! returned stream after successful spawning; subsequent executions have no
//! input. Output writers also move into graph streams after spawning; subsequent
//! calls on that wrapper discard stdout. Other stream-input wrappers consume input from its current position;
//! the prompter resends its stored prompt. None rewind stream input.
//! Once early child completion is observed, any pending feed is cancelled.
//! A zero exit status with an unfinished feed produces
//! [`ExecutorError::IncompleteInput`](crate::ExecutorError::IncompleteInput), not
//! silent success. For intentional early exit, the low-level
//! [`Executor::execute_with_io_completion`](crate::Executor::execute_with_io_completion)
//! returns separate process and input outcomes. Convenience APIs use the error
//! precedence in [`ExecutionCompletion::into_result`](crate::ExecutionCompletion::into_result):
//! source errors and non-broken-pipe stdin errors precede exit errors, which
//! precede broken-pipe or incomplete-input errors. Transport/forwarding and wait
//! failures are returned directly. Successful input delivery confirms bytes
//! reached the pipe, not that the child processed them at the application level.
//!
//! [`Writer`] retains arbitrary-format, buffered output; [`Compiler`] and
//! [`Runner`] also return in-memory cursors. [`Indexer`] discards stdout and
//! returns `()` on success. [`Prompter`] decodes captured stdout as UTF-8 text;
//! [`Resolver`] parses it as ordered, validated absolute URL lines, preserving
//! spelling and duplicates. Both buffer captured output before decoding.
//! [`Output::Captured`](crate::Output::Captured) returns the payload. Ignored,
//! inherited, or forwarded output returns an empty cursor, stream, string, or
//! vector as appropriate, while still checking execution success.
//!
//! [`Output::AsyncWrite`](crate::Output::AsyncWrite) forwards stdout incrementally
//! with backpressure and flushes the writer at EOF, without shutting it down or
//! also capturing the bytes. Write/flush failures fail execution and terminate
//! the child. Graph streams drive forwarding when polled; buffered wrappers
//! await it and retain their writer for reuse. Forwarded bytes are not decoded.
//! All wrappers capture stderr without a size bound for
//! [`ExecutorError`](crate::ExecutorError) diagnostics on unsuccessful exits.
//! Successful stderr is discarded by convenience APIs; detailed completions
//! retain it. Invalid UTF-8 diagnostics are omitted from exit-error messages.
//! Dropping an in-progress execution future drops its owned child handle and,
//! under the executor's default kill-on-drop policy, requests termination.
//! The same applies to dropping a returned graph stream, including its upstream
//! input streams when programs are connected together.
//! Cancelling buffered execution retains its input in the wrapper; drop that
//! wrapper to also release any owned upstream streams. Termination applies to
//! each owned child, without guaranteeing termination of descendant processes.
//! Cancellation does not report success or roll back external side effects.
//! [`Pipeline`](crate::Pipeline) composes graph producers and consumers with
//! direct OS pipes, checks every stage, and coordinates failure cleanup. Its
//! limited-lister source uses a bounded relay to preserve the local line cap.
//! Pipeline construction consumes configured wrappers; external stdin belongs
//! to the first stage and the final stage's output policy selects the result.
//!
//! All subprocess I/O is awaited within execution or the returned stream;
//! prompt writing does not use a detached task. Buffered captures, individual
//! JSONL lines, and captured stderr have no configured size bound.
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
