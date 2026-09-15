// This is free and unencumbered software released into the public domain.

//! Role-specific execution traits and command-line configuration values.
//!
//! Each trait is a marker extending [`Execute`](crate::Execute). It identifies
//! the intended operation but adds no constructor, input parameter, or runtime
//! validation. `T` and `E` are the implementation's success and error types;
//! [`Indexer`] uses `()` for success because it has no payload output.
//!
//! # Pattern catalog
//!
//! | Trait | Logical input | Logical output | Options |
//! | --- | --- | --- | --- |
//! | [`Adapter`] | SPARQL query | RDF query result | [`AdapterOptions`] |
//! | [`Compiler`] | Natural-language text | SPARQL query | [`CompilerOptions`] |
//! | [`Emitter`] | None | Generated RDF | [`EmitterOptions`] |
//! | [`Fetcher`] | One URL | RDF describing the resource | [`FetcherOptions`] |
//! | [`Indexer`] | RDF | None; updates a persistent index | [`IndexerOptions`] |
//! | [`Lister`] | One collection URL | RDF describing zero or more entries | [`ListerOptions`] |
//! | [`Matcher`] | RDF | RDF describing matches | [`MatcherOptions`] |
//! | [`Prompter`] | Prompt text | Response text | [`PrompterOptions`] |
//! | [`Reader`] | Document or byte stream | Imported RDF | [`ReaderOptions`] |
//! | [`Reasoner`] | RDF | Entailed RDF | [`ReasonerOptions`] |
//! | [`Resolver`] | One URI | Zero or more URLs | [`ResolverOptions`] |
//! | [`Runner`] | Program text | Execution result as text | [`RunnerOptions`] |
//! | [`Writer`] | RDF | Exported document or byte stream | [`WriterOptions`] |
//!
//! All thirteen patterns in the [specification][pps] have corresponding traits
//! here and process wrappers in `asimov-runner`. RDF in this table denotes
//! graphs or datasets, not individual statements, records, or buffers.
//!
//! # Options and defaults
//!
//! Every options type supports `Default`, direct field access, and a
//! `builder()`. Defaults leave optional fields unset and collections empty;
//! builders accept values convertible to `String` for string fields. Values
//! are stored without checking whether the selected program supports them.
//!
//! The companion [`asimov-runner`][runner] wrappers emit configured fields as
//! individual `--name=value` arguments. `None` omits an option rather than
//! supplying an empty value. A conforming program then applies these defaults:
//!
//! | Patterns | Input format | Output format | Other defaults |
//! | --- | --- | --- | --- |
//! | Adapter, emitter, fetcher | No input-format option | `jsonl` | — |
//! | Compiler | No input-format option | No output-format option | No pattern-specific options |
//! | Indexer | `jsonl` | No output-format option | Index destination required |
//! | Lister | No input-format option | `jsonl` | No limit; offset `0`; program-defined order |
//! | Matcher, reasoner | `jsonl` | `jsonl` | — |
//! | Prompter | `text` | `text` | Model `auto` |
//! | Reader | `auto` | `jsonl` | — |
//! | Resolver | No input-format option | No output-format option | No limit |
//! | Runner | No input-format option | No output-format option | No definitions |
//! | Writer | `jsonl` | `auto` | — |
//!
//! `text` means UTF-8 without a standardized chat envelope. `auto` delegates
//! format detection or selection to the program. `jsonl` needs a documented
//! [RDF mapping profile][rdf-mapping] shared by producer and consumer. Setting
//! a format option does not encode, decode, or convert any bytes in this crate.
//!
//! # Additional arguments and files
//!
//! Each `other` vector is an ordered argument list, not a shell command. The
//! runner appends its entries after generated options and before a dedicated
//! URL or URI operand. Each entry is passed verbatim as one argument: use two
//! entries for `--name value`, or one for `--name=value`. Do not add shell quotes
//! or redirection syntax. Repeated builder `other(...)` calls append; the
//! `maybe_other(...)` helpers, where available, append only `Some` values.
//!
//! Additional arguments can supply documented extensions or positional files.
//! Keep options before operands; use `--` to end option parsing when a filename
//! begins with `-`. Avoid repeating singleton options already set in fields:
//! precedence is program-defined, so `other` is not an override mechanism.
//!
//! **Format selection and file selection are separate.** For patterns with
//! optional input/output files, no operands select stdin/stdout, one operand
//! selects the input file, and two select input then output. To read stdin and
//! write a file, supply `-` followed by the output path. An indexer instead
//! requires its final operand to name the persistent index. See each trait's
//! synopsis for the applicable operand rules.
//!
//! With a named input file, configure a stream-input wrapper's input as ignored:
//! the file replaces stdin as the payload source. The prompter instead always
//! writes its stored prompt; see [`PrompterOptions::other`] for that limitation.
//! Selecting an output file does not arrange for the wrapper to read that file
//! back. Capturing stdout in that case does not capture the file's contents.
//!
//! [pps]: https://asimov-specs.github.io/program-patterns/#patterns
//! [runner]: https://docs.rs/asimov-runner/latest/asimov_runner/programs/
//! [rdf-mapping]: https://asimov-specs.github.io/program-patterns/#rdf-mapping

mod adapter;
pub use adapter::*;

mod compiler;
pub use compiler::*;

mod lister;
pub use lister::*;

mod emitter;
pub use emitter::*;

mod fetcher;
pub use fetcher::*;

mod indexer;
pub use indexer::*;

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
