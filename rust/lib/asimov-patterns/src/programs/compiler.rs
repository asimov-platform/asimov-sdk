// This is free and unencumbered software released into the public domain.

//! Natural-language query compilation: the compiler marker trait and arguments.

use crate::Execute;
use alloc::{string::String, vec::Vec};
use bon::Builder;

/// A prompt compiler that translates natural-language text into one SPARQL query.
///
/// The query is intended for an [`Adapter`](crate::Adapter); compiling it does
/// not execute it. The program documents its assumptions about the target
/// dataset, vocabulary, and adapter capabilities. Syntactic validity alone does
/// not guarantee that a query captures the author's intent.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE]`
///
/// The natural-language input file defaults to `-` (stdin). On success, stdout
/// contains a syntactically valid SPARQL query as UTF-8, without Markdown fences
/// or explanatory prose outside the query. The query must produce a graph or
/// use a form for which the intended adapter's profile defines an RDF mapping.
/// There are no standard pattern-specific options or output-file operand.
///
/// `T` is the implementation's query representation. See [`crate::programs`]
/// for links to concrete execution behavior and the [compiler specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#compiler
pub trait Compiler<T>: Execute<T> {}

/// Additional arguments and an optional input file for a [`Compiler`].
///
/// `Default` creates an empty argument list, selecting the program's stdin-input
/// form. The pattern defines no standard model or format options. Any such
/// options supplied through [`other`](Self::other) are extensions requiring
/// support from the selected program; this type does not validate that support.
///
/// # Examples
///
/// Select a named input file as one literal argument, retaining stdout output:
///
/// ```
/// use asimov_patterns::CompilerOptions;
///
/// let options = CompilerOptions::builder()
///     .other("--")
///     .other("request with spaces.txt")
///     .build();
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct CompilerOptions {
    /// Literal command-line arguments, including an optional input-file operand.
    ///
    /// The process wrapper forwards these in order and generates no other
    /// arguments. Put extension options before the input file. Each string is
    /// one argument, without shell expansion; see [`crate::programs`]. With a
    /// named file, configure the wrapper's stream input as ignored.
    #[builder(field)]
    pub other: Vec<String>,
}

impl<S: compiler_options_builder::State> CompilerOptionsBuilder<S> {
    /// Appends one literal argument to [`CompilerOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`CompilerOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }
}
