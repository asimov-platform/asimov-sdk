// This is free and unencumbered software released into the public domain.

//! URL-to-RDF retrieval: the fetcher marker trait and options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// A URL protocol client that retrieves one resource and represents it as RDF.
///
/// The program defines supported URL schemes and the mapping from retrieved
/// content to RDF. A single resource may produce many RDF statements. Returning
/// arbitrary response bytes alone does not fulfill the RDF output contract.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] INPUT-URL`
///
/// One absolute URL is required as a single argument; it is not a stdin
/// payload or an implicitly converted local pathname. RDF is written to stdout
/// using [`FetcherOptions::output`] (`jsonl` by default). The program validates
/// the URL and rejects unsupported schemes.
///
/// `T` is the implementation's result representation. Retrieval, redirects,
/// authentication, caching, and resource-to-RDF mapping belong to the program.
/// See [`crate::programs`] for links to concrete execution behavior and the
/// [fetcher specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#fetcher
pub trait Fetcher<T>: Execute<T> {}

/// Output-format selection and additional arguments for a [`Fetcher`].
///
/// The URL is supplied separately by the implementation's invocation API.
/// `Default` leaves `output` unset and `other` empty; format support and URL
/// validity are not checked by this configuration type.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::FetcherOptions;
///
/// let options = FetcherOptions::builder()
///     .output("jsonl")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct FetcherOptions {
    /// Additional arguments placed after generated options and before the URL.
    ///
    /// Each string is one literal argument, without shell expansion. The runner
    /// supplies the URL separately; do not duplicate it here. See [`crate::programs`].
    #[builder(field)]
    pub other: Vec<String>,

    /// RDF serialization passed as `--output=FORMAT` (`-o` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This names a serialization, not a file or a raw-response retrieval mode.
    pub output: Option<String>,
}

impl<S: fetcher_options_builder::State> FetcherOptionsBuilder<S> {
    /// Appends one literal argument to [`FetcherOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`FetcherOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
