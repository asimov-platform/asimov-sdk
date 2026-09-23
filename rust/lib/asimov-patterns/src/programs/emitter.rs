// This is free and unencumbered software released into the public domain.

//! Input-free RDF generation: the emitter marker trait and options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// A value generator that produces RDF without consuming a payload input.
///
/// Arguments, configuration, the environment, or external sources determine
/// the generated values. Generation may be finite or continuous and need not
/// be deterministic. A finite emitter can succeed with an empty RDF result.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS]`
///
/// There are no standard positional operands, and the program must not wait
/// for stdin. RDF is written to stdout in the format selected by
/// [`EmitterOptions::output`] (`jsonl` by default).
///
/// Implementations document termination conditions and how `T` exposes output.
/// See [`crate::programs`] for shared conventions and links to concrete execution
/// behavior, and the
/// [emitter specification][spec] for the external contract.
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#emitter
pub trait Emitter<T>: Execute<T> {}

/// Output-format selection and additional arguments for an [`Emitter`].
///
/// `Default` requests the program's defaults by leaving `output` unset and
/// `other` empty. These options do not define the generated values, bound the
/// execution time, or validate the selected program's capabilities.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::EmitterOptions;
///
/// let options = EmitterOptions::builder()
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct EmitterOptions {
    /// Additional arguments appended after the generated output-format option.
    ///
    /// Each string is one literal argument, without shell expansion. The
    /// standard pattern has no positional operands; extensions require support
    /// from the selected program. See [`crate::programs`].
    #[builder(field)]
    pub other: Vec<String>,

    /// RDF serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This does not select a file, capture policy, or RDF mapping profile.
    pub output: Option<String>,
}

impl<S: emitter_options_builder::State> EmitterOptionsBuilder<S> {
    /// Appends one literal argument to [`EmitterOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`EmitterOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
