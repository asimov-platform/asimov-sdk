// This is free and unencumbered software released into the public domain.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// Exact or approximate matcher. Consumes RDF input, produces RDF output describing matches.
///
/// See: https://asimov-specs.github.io/program-patterns/#matcher
pub trait Matcher<T, E>: Execute<T, E> {}

/// Configuration options for [`Matcher`].
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
    /// Extended nonstandard matcher options.
    #[builder(field)]
    pub other: Vec<String>,

    /// The input format.
    pub input: Option<String>,

    /// The output format.
    pub output: Option<String>,
}

impl<S: matcher_options_builder::State> MatcherOptionsBuilder<S> {
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
