// This is free and unencumbered software released into the public domain.

//! RDF entailment: the reasoner marker trait and serialization options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// An RDF entailer that derives consequences under a documented regime or rule system.
///
/// The program defines whether output includes the original statements or
/// only additional consequences, and how named graphs and inconsistent input
/// are handled. The pattern specifies no particular reasoning algorithm, rule
/// language, or completeness guarantee.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE [OUTPUT-FILE]]`
///
/// Input and output default to stdin and stdout. A single operand selects
/// input; two select input then output, with `-` denoting a standard stream.
/// [`ReasonerOptions`] selects RDF serializations, both defaulting to `jsonl`.
///
/// `T` is the implementation's result representation. The `asimov-runner`
/// wrapper returns captured bytes without interpreting the inference result or
/// checking entailment. See [`crate::programs`] and the
/// [reasoner specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#reasoner
pub trait Reasoner<T, E>: Execute<T, E> {}

/// RDF input/output formats and additional arguments for a [`Reasoner`].
///
/// `Default` leaves formats unset and `other` empty. The process wrapper omits
/// unset flags and lets the program apply its defaults. These options do not
/// select a standard entailment regime or validate the RDF payloads.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::ReasonerOptions;
///
/// let options = ReasonerOptions::builder()
///     .input("jsonl")
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct ReasonerOptions {
    /// Additional arguments, including optional input and output file operands.
    ///
    /// The wrapper appends these after generated format options. Put extension
    /// options before files; each string is one literal argument. See
    /// [`crate::programs`] for operand ordering and standard-stream selection.
    #[builder(field)]
    pub other: Vec<String>,

    /// RDF input serialization passed as `--input=FORMAT` (`-i` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This is a format token, not an input filename or a rule language.
    pub input: Option<String>,

    /// Entailed RDF serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This selects neither an output file nor whether input statements are included.
    pub output: Option<String>,
}

impl<S: reasoner_options_builder::State> ReasonerOptionsBuilder<S> {
    /// Appends one literal argument to [`ReasonerOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`ReasonerOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
