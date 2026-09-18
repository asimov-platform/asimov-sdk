// This is free and unencumbered software released into the public domain.

//! RDF dataset import through an external reader program.

use crate::{AnyInput, Executor, ExecutorError, GraphOutput, JsonlStream};
use alloc::{boxed::Box, format, vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::ReaderOptions;

/// A live JSONL graph stream, or an error starting the reader.
pub type ReaderResult = Result<JsonlStream, ExecutorError>;

/// An external [reader] that imports input data into an RDF dataset.
///
/// This is a program-pattern wrapper, not an implementation of an I/O reader
/// trait. Input parsing and conversion are performed by the external program.
/// Execution uses the concurrent streaming behavior described in
/// [`crate::programs`].
///
/// [reader]: https://asimov-specs.github.io/program-patterns/#reader
#[allow(unused)]
#[derive(Debug)]
pub struct Reader {
    executor: Executor,
    options: ReaderOptions,
    input: AnyInput,
    output: GraphOutput,
}

impl Reader {
    /// Configures a reader without starting it.
    ///
    /// Adds any configured `--input=<format>` and `--output=<format>` arguments,
    /// followed by `options.other`. The input and output values select stdin
    /// and stdout; stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: AnyInput,
        output: GraphOutput,
        options: ReaderOptions,
    ) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(ref input) = options.input {
                vec![format!("--input={}", input)]
            } else {
                vec![]
            })
            .args(if let Some(ref output) = options.output {
                vec![format!("--output={}", output)]
            } else {
                vec![]
            })
            .args(&options.other)
            .stdin(input.as_stdio())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    /// Starts a child and returns its live JSONL graph stream.
    ///
    /// After successful spawning, input ownership moves into the stream, which
    /// feeds it concurrently when polled. Subsequent executions have no source input.
    ///
    /// # Errors
    ///
    /// Spawn failures are returned directly; input, read, wait, and exit failures are
    /// stream items. Consume the stream to completion to check process success.
    pub async fn execute(&mut self) -> ReaderResult {
        self.executor
            .execute_jsonl_with_input(&mut self.input)
            .await
    }
}

impl asimov_patterns::Reader<JsonlStream, ExecutorError> for Reader {}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream, ExecutorError> for Reader {
    async fn execute(&mut self) -> ReaderResult {
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
