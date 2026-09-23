// This is free and unencumbered software released into the public domain.

//! Dataset queries: the adapter marker trait and output-format options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// An RDF dataset proxy that evaluates a SPARQL query and produces RDF.
///
/// The program defines the dataset and supported SPARQL features. `CONSTRUCT`
/// and `DESCRIBE` yield graph results; `SELECT` and `ASK` need a documented
/// mapping of their results to RDF. The pattern does not define SPARQL Update.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [QUERY-FILE]`
///
/// The query file defaults to `-` (stdin); RDF is written to stdout. Output
/// serialization is selected by [`AdapterOptions::output`] and defaults to
/// `jsonl`. Unsupported query forms must cause failure rather than a non-RDF
/// result mislabeled as RDF.
///
/// `T` represents the implementation's result, not necessarily a parsed graph.
/// See [`crate::programs`] for shared options and links to concrete execution
/// behavior, and the
/// [adapter specification][spec] for the external program's requirements.
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#adapter
pub trait Adapter<T>: Execute<T> {}

/// Output-format selection and additional arguments for an [`Adapter`].
///
/// `Default` leaves the format unset and additional arguments empty. The
/// process wrapper omits unset options, delegating defaults to the program;
/// building this value does not parse the query or validate format support.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::AdapterOptions;
///
/// let options = AdapterOptions::builder()
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct AdapterOptions {
    /// Additional arguments, in order, including an optional query-file operand.
    ///
    /// The process wrapper appends these after `--output`. Each string is one
    /// literal argument, without shell expansion; see [`crate::programs`].
    #[builder(field)]
    pub other: Vec<String>,

    /// RDF serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This selects a format, not an output filename or capture policy.
    pub output: Option<String>,
}

impl<S: adapter_options_builder::State> AdapterOptionsBuilder<S> {
    /// Appends one literal argument to [`AdapterOptions::other`].
    ///
    /// Calls accumulate in order. A separate option value needs its own call;
    /// for example, `.other("--name").other("value with spaces")`.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`AdapterOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_arguments_preserve_order_and_boundaries() {
        let options = AdapterOptions::builder()
            .other("--dataset")
            .maybe_other(Some("value with spaces"))
            .maybe_other(None::<&str>)
            .other("query.rq")
            .build();
        assert_eq!(
            options.other,
            ["--dataset", "value with spaces", "query.rq"]
        );
    }
}
