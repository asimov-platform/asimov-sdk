// This is free and unencumbered software released into the public domain.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;
use clientele::options::sort::SortKeys;

/// Graph iterator. Takes a URL input, produces RDF output.
///
/// See: https://asimov-specs.github.io/program-patterns/#cataloger
pub trait Cataloger<T, E>: Execute<T, E> {}

/// Configuration options for [`Cataloger`].
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::CatalogerOptions;
///
/// let options = CatalogerOptions::builder()
///     .limit(100)
///     .output("jsonld")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, /*Ord,*/ PartialEq, /*PartialOrd,*/ Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct CatalogerOptions {
    /// Extended nonstandard cataloger options.
    #[builder(field)]
    pub other: Vec<String>,

    /// Sort resources by the specified keys. (Prefix a key with `-` for descending order.)
    pub sort: Option<SortKeys>,

    /// The index offset of the first output.
    pub offset: Option<usize>,

    /// The maximum count of outputs.
    pub limit: Option<usize>,

    /// The output format.
    pub output: Option<String>,
}

impl<S: cataloger_options_builder::State> CatalogerOptionsBuilder<S> {
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
