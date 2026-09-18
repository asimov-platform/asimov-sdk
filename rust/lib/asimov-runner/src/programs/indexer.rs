// This is free and unencumbered software released into the public domain.

//! Persistent RDF dataset indexing through an external indexer program.

use crate::{CommandExt, Executor, ExecutorError, GraphInput, NoOutput};
use alloc::boxed::Box;
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::IndexerOptions;

/// Successful completion of an [`Indexer`], or an execution error.
///
/// Success carries no output value; the indexer's stdout is discarded.
pub type IndexerResult = std::result::Result<(), ExecutorError>;

/// An external [indexer] that consumes RDF to maintain a persistent index.
///
/// JSONL lines are fed to stdin without parsing or validation. Index storage
/// and update semantics are the external program's responsibility. Stdout is
/// discarded, and a successful process exit produces `()`. Input handling
/// follows the behavior described in [`crate::programs`].
///
/// [indexer]: https://asimov-specs.github.io/program-patterns/#indexer
#[allow(unused)]
#[derive(Debug)]
pub struct Indexer {
    executor: Executor,
    options: IndexerOptions,
    input: GraphInput,
    output: NoOutput,
}

impl Indexer {
    /// Configures an indexer without starting it.
    ///
    /// Adds `--input=<format>` when `options.input` is set, followed by
    /// `options.other`. `input` selects stdin handling; stdout is discarded and
    /// stderr is captured for failure diagnostics.
    /// Byte input is lazily adapted into JSONL lines using [`GraphInput::into_jsonl`].
    ///
    /// The specification requires an `INDEX-FILE` positional argument. This
    /// constructor has no dedicated index-path parameter; include that argument
    /// in `options.other`. With no explicit input-file argument, the program is
    /// expected to read its RDF input from stdin.
    ///
    /// For example, `IndexerOptions::builder().other("./catalog.index").build()`
    /// selects an index destination while retaining stdin input. With two file
    /// operands, input precedes the index path. The index path cannot be `-`;
    /// use `./-` if that is the literal filename. This constructor does not
    /// validate operand count or index-path semantics.
    pub fn new(program: impl AsRef<OsStr>, input: GraphInput, options: IndexerOptions) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
            .option("input", options.input.as_ref())
            .args(&options.other)
            .stdin(input.as_stdio())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input: input.into_jsonl(),
            output: (),
        }
    }

    /// Sends the remaining graph input to a new child and waits for indexing to finish.
    ///
    /// Input is fed concurrently with draining stderr. Early child completion can
    /// interrupt the input feed and fail execution even on a zero exit status;
    /// see [`Executor::execute_with_input`].
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the indexer exits unsuccessfully.
    pub async fn execute(&mut self) -> IndexerResult {
        let _stdout = self.executor.execute_with_input(&mut self.input).await?;
        Ok(())
    }
}

impl asimov_patterns::Indexer for Indexer {}

crate::pipeline::stage!(
    Indexer,
    value,
    value.input,
    crate::Output::Ignored,
    value.options.input.as_deref(),
    None
);

#[async_trait]
impl asimov_patterns::Execute<()> for Indexer {
    type Error = ExecutorError;

    async fn execute(&mut self) -> IndexerResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    //use super::*;
    //use asimov_patterns::Execute;

    #[tokio::test]
    async fn test_execute() {
        // TODO
    }
}
