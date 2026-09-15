// This is free and unencumbered software released into the public domain.

//! URI resolution: the resolver marker trait and URL-result limits.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// A URI resolver that identifies zero or more resource locations as URLs.
///
/// Input can be a URN or URL. Resolution identifies locations; it does not
/// retrieve the resources at those locations. The program defines supported
/// schemes, result order, and duplicate handling. No matches is a valid result.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] INPUT-URI`
///
/// One absolute URI is required as a single argument; omission does not select
/// stdin. [`ResolverOptions::limit`] bounds the number of URLs. The program
/// writes zero or more UTF-8 absolute URLs to stdout, one per LF-terminated
/// line, with no blank records, header, or JSON envelope. Zero results means
/// zero output bytes.
///
/// Hosts parsing these records preserve order and accept CRLF and a final
/// nonempty line without a terminator. Removing line endings does not authorize
/// trimming other whitespace, percent-decoding, or splitting on commas/spaces.
///
/// `T` is the implementation's result representation, not necessarily one URL.
/// **The current `asimov-runner` wrapper does not parse resolver output:** it
/// returns an empty vector after any successful process exit, even if stdout
/// contained results. That value cannot establish that resolution found no
/// locations. See [`crate::programs`] and the [resolver specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#resolver
pub trait Resolver<T, E>: Execute<T, E> {}

/// Result-count selection and additional arguments for a [`Resolver`].
///
/// The URI is supplied separately by the runner. `Default` leaves `limit`
/// unset and `other` empty, imposing no caller-requested limit. The standard
/// output framing is fixed; this type has no output-format field.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::ResolverOptions;
///
/// let options = ResolverOptions::builder()
///     .limit(100)
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct ResolverOptions {
    /// Additional arguments placed after the generated limit and before the URI.
    ///
    /// Each string is one literal argument, without shell expansion. The runner
    /// supplies the URI separately; do not duplicate it here. See [`crate::programs`].
    #[builder(field)]
    pub other: Vec<String>,

    /// Maximum number of output URLs, passed as `--limit=COUNT` (`-n` in the CLI).
    ///
    /// `None` imposes no caller-requested limit; `Some(0)` requests no results.
    /// A zero limit may still trigger validation or resource-availability checks.
    /// The runner forwards a decimal integer without checking the program's
    /// supported range; this is not a byte limit or a timeout.
    pub limit: Option<usize>,
}

impl<S: resolver_options_builder::State> ResolverOptionsBuilder<S> {
    /// Appends one literal argument to [`ResolverOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`ResolverOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
