// This is free and unencumbered software released into the public domain.

//! Persistent RDF indexing: the indexer marker trait and input options.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// An RDF consumer that creates or updates a persistent index without payload output.
///
/// The program defines its index format, creation and update semantics,
/// concurrency behavior, and guarantees on failure. `Ok(())` represents
/// successful completion under those guarantees; it does not independently
/// promise a transaction, crash durability, or safe replay after failure.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE] INDEX-FILE`
///
/// With one operand, that operand is the index destination and input is stdin.
/// With two, the first selects the input file (`-` for stdin) and the second
/// selects the destination. An index destination is required and can be a file
/// or directory according to the program. It cannot be `-`; use `./-` for a
/// literal path of that name. Stdout carries no payload.
///
/// [`IndexerOptions::input`] selects the RDF format (`jsonl` by default).
/// [`IndexerOptions::other`] can supply the destination operand. See
/// [`crate::programs`] for links to concrete execution behavior and the
/// [indexer specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#indexer
pub trait Indexer: Execute<()> {}

/// Input-format selection and additional arguments for an [`Indexer`].
///
/// `Default` leaves `input` unset and `other` empty. It does **not** constitute
/// a complete command-line invocation: the required index destination must
/// still be supplied. The example configures the runner's stdin-input form.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::IndexerOptions;
///
/// let options = IndexerOptions::builder()
///     .input("jsonl")
///     .other("./catalog.index")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct IndexerOptions {
    /// Additional arguments, including the required index-destination operand.
    ///
    /// The process wrapper appends these after `--input`. Put extension options
    /// first, then either the index path alone or input path followed by index
    /// path. Each string is one literal argument; see [`crate::programs`].
    #[builder(field)]
    pub other: Vec<String>,

    /// RDF serialization passed as `--input=FORMAT` (`-i` in the CLI).
    ///
    /// `None` omits the option; the specified program default is `jsonl`.
    /// This selects neither the input file nor the persistent index format.
    pub input: Option<String>,
}

impl<S: indexer_options_builder::State> IndexerOptionsBuilder<S> {
    /// Appends one literal argument to [`IndexerOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`IndexerOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
