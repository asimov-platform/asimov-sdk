// This is free and unencumbered software released into the public domain.

//! RDF export: the writer marker trait and input/destination format options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// An exporter that converts an RDF graph or dataset into a supported representation.
///
/// Output may be another RDF serialization or a non-RDF document or byte stream.
/// The program defines supported formats, mappings, and any information loss,
/// such as omission of named graphs or datatype information. This role describes
/// a data-export operation, not an implementation of Rust's byte-writing traits.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE [OUTPUT-FILE]]`
///
/// Input and output default to stdin and stdout. A single operand selects
/// input; two select input then output, with `-` denoting a standard stream.
/// [`WriterOptions`] defaults to `jsonl` input and automatic output-format
/// selection. The program must define automatic selection even when stdout
/// has no filename from which to infer a format.
///
/// `T` is the implementation's exported-result representation. See
/// [`crate::programs`] for links to concrete execution behavior and the
/// [writer specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#writer
pub trait Writer<T>: Execute<T> {}

/// RDF input and export formats for a [`Writer`], plus additional arguments.
///
/// `Default` leaves formats unset and `other` empty, delegating defaults to
/// the program. Select an explicit output format when the consumer requires
/// a particular representation. Format selection itself does not convert data
/// or establish that the selected program supports that format.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::WriterOptions;
///
/// let options = WriterOptions::builder()
///     .input("jsonl")
///     .output("auto")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct WriterOptions {
    /// Additional arguments, including optional RDF input and exported output files.
    ///
    /// The wrapper appends these after generated format options. Put extension
    /// options before files; each string is one literal argument. See
    /// [`crate::programs`] for operand ordering and standard-stream selection.
    #[builder(field)]
    pub other: Vec<String>,

    /// RDF input serialization passed as `--input=FORMAT` (`-i` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This is a format token, not an input filename.
    pub input: Option<String>,

    /// Export format passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified default is `auto`, requesting the
    /// program's documented format-selection policy. `auto` is not itself a
    /// serialization. This field selects neither an output file nor capture behavior.
    pub output: Option<String>,
}

impl<S: writer_options_builder::State> WriterOptionsBuilder<S> {
    /// Appends one literal argument to [`WriterOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`WriterOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
