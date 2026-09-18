// This is free and unencumbered software released into the public domain.

//! RDF matching: the matcher marker trait and serialization options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// An exact or approximate matcher that consumes RDF and describes matches as RDF.
///
/// The output need not be a subset of the input: it can introduce statements
/// describing correspondences or scores. The program defines the matching
/// relation, compared entities or reference data, output vocabulary, and score
/// interpretation. The pattern imposes no universal algorithm or score scale.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE [OUTPUT-FILE]]`
///
/// Input and output default to stdin and stdout. A single operand selects
/// input; two select input then output, with `-` denoting a standard stream.
/// [`MatcherOptions`] selects serializations, both defaulting to `jsonl`.
///
/// `T` represents the result in the implementation's chosen form. The
/// `asimov-runner` wrapper feeds JSONL input concurrently with returning a live,
/// fallible stream of JSONL byte-vector lines, without interpreting matches.
/// Consume the stream to completion to check process success.
/// See [`crate::programs`] and the [matcher specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#matcher
pub trait Matcher<T, E>: Execute<T, E> {}

/// Input/output formats and additional arguments for a [`Matcher`].
///
/// `Default` leaves formats unset and `other` empty. The process wrapper omits
/// unset flags and lets the program apply its defaults. This type selects no
/// matching algorithm and does not validate or transform RDF.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::MatcherOptions;
///
/// let options = MatcherOptions::builder()
///     .input("jsonl")
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct MatcherOptions {
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
    /// This is a format token, not an input filename.
    pub input: Option<String>,

    /// RDF match-result serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This selects neither an output file nor the vocabulary describing matches.
    pub output: Option<String>,
}

impl<S: matcher_options_builder::State> MatcherOptionsBuilder<S> {
    /// Appends one literal argument to [`MatcherOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`MatcherOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
