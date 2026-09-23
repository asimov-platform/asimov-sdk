// This is free and unencumbered software released into the public domain.

//! RDF import: the reader marker trait and source/output format options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// An importer that maps a document or byte stream into an RDF graph or dataset.
///
/// Source data may use an RDF or non-RDF format. The program defines supported
/// inputs and their mappings, including vocabulary, base-IRI handling, and
/// identifier generation. This role describes a data-import operation, not an
/// implementation of Rust's byte-reading traits.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE [OUTPUT-FILE]]`
///
/// Input and output default to stdin and stdout. A single operand selects
/// input; two select input then output, with `-` denoting a standard stream.
/// [`ReaderOptions`] defaults to automatic input-format detection and `jsonl`
/// output. The program must document its detection procedure and fail if it
/// cannot select a supported input format.
///
/// `T` is the implementation's imported-result representation. See
/// [`crate::programs`] for links to concrete execution behavior and the
/// [reader specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#reader
pub trait Reader<T>: Execute<T> {}

/// Source and RDF output formats for a [`Reader`], plus additional arguments.
///
/// `Default` leaves formats unset and `other` empty, delegating detection and
/// output defaults to the program. Configuration does not perform detection,
/// validate format support, or transform the input.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::ReaderOptions;
///
/// let options = ReaderOptions::builder()
///     .input("auto")
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct ReaderOptions {
    /// Additional arguments, including optional source and RDF output file operands.
    ///
    /// The wrapper appends these after generated format options. Put extension
    /// options before files; each string is one literal argument. See
    /// [`crate::programs`] for operand ordering and standard-stream selection.
    #[builder(field)]
    pub other: Vec<String>,

    /// Source format passed as `--input=FORMAT` (`-i` in the CLI).
    ///
    /// `None` omits the option; the specified default is `auto`, requesting the
    /// program's documented detection procedure. An explicit concrete format
    /// takes precedence over detection. This is not an input filename.
    pub input: Option<String>,

    /// RDF serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This selects a serialization, not an output file or a source-to-RDF mapping.
    pub output: Option<String>,
}

impl<S: reader_options_builder::State> ReaderOptionsBuilder<S> {
    /// Appends one literal argument to [`ReaderOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`ReaderOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
