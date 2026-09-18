// This is free and unencumbered software released into the public domain.

//! Language-runtime execution: the runner marker trait and named definitions.

use crate::Execute;
use alloc::{collections::btree_map::BTreeMap, string::String, vec::Vec};
use bon::Builder;

/// A language runtime that executes program text and emits its result as text.
///
/// The program defines its language and version, input grammar and encoding,
/// how definitions are exposed to code, and how results are represented.
/// Executing code can have external side effects; the pattern does not imply
/// sandboxing, determinism, or rollback after failure.
///
/// # Command-line contract
///
/// `PROGRAM [OPTIONS] [INPUT-FILE]`
///
/// The program file defaults to `-` (stdin); the execution result goes to
/// stdout. [`RunnerOptions::define`] supplies runtime variables through the
/// repeatable `--define=VAR=VAL` option. No standard input/output-format
/// options or output-file operand are defined for this pattern.
///
/// `T` is the implementation's result representation. This trait identifies the
/// language-runtime role, not generic process launching. See [`crate::programs`]
/// for links to concrete execution behavior and the [runner specification][spec].
///
/// [spec]: https://asimov-specs.github.io/program-patterns/#runner
pub trait Runner<T>: Execute<T> {}

/// Runtime variable definitions and additional arguments for a [`Runner`].
///
/// `Default` creates an empty definition map and argument list. The map holds
/// at most one value per name and is forwarded in key order, not insertion
/// order. Values are stored without validating the selected runtime's naming
/// or value rules.
///
/// # Examples
///
/// ```rust
/// use asimov_patterns::RunnerOptions;
///
/// let options = RunnerOptions::builder()
///     .define("mode", "preview")
///     .define("expression", "a=b")
///     .build();
///
/// assert_eq!(options.define.get("expression").map(String::as_str), Some("a=b"));
/// ```
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Builder)]
#[builder(derive(Debug), on(String, into))]
pub struct RunnerOptions {
    /// Additional arguments appended after definitions, including an optional program file.
    ///
    /// Each string is one literal argument, without shell expansion. To express
    /// ordered or repeated `--define` arguments that the map cannot represent,
    /// put them here and leave those keys out of [`define`](Self::define).
    /// The program's documented duplicate-definition policy then applies.
    /// See [`crate::programs`] for option and operand ordering.
    #[builder(field)]
    pub other: Vec<String>,

    /// Named runtime values passed as `--define=VAR=VAL` (`-D` in the CLI).
    ///
    /// The runner emits one argument per entry in [`BTreeMap`] key order. The
    /// program splits the argument value at its first `=`: names must be
    /// nonempty and cannot contain `=`, while values may be empty or contain
    /// additional `=` characters. These constraints are not checked here.
    ///
    /// Replacing a key in this map replaces its previous value before execution;
    /// it does not send duplicate definitions to the program. Definitions are
    /// runtime arguments, not environment-variable assignments or shell source.
    #[builder(field)]
    pub define: BTreeMap<String, String>,
}

impl<S: runner_options_builder::State> RunnerOptionsBuilder<S> {
    /// Appends one literal argument to [`RunnerOptions::other`], preserving order.
    pub fn other(mut self, flag: impl Into<String>) -> Self {
        self.other.push(flag.into());
        self
    }

    /// Appends a present argument to [`RunnerOptions::other`]; `None` adds nothing.
    pub fn maybe_other(mut self, flag: Option<impl Into<String>>) -> Self {
        if let Some(flag) = flag {
            self.other.push(flag.into());
        }
        self
    }

    /// Inserts a runtime definition, replacing any earlier value for the same key.
    ///
    /// Calls for different keys accumulate in [`RunnerOptions::define`], which
    /// is emitted in key order. Neither the name nor the value is validated.
    pub fn define(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.define.insert(key.into(), val.into());
        self
    }
}
