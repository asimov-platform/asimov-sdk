// This is free and unencumbered software released into the public domain.

//! RDF dataset import through an external reader program.

use crate::{AnyInput, Executor, ExecutorError, GraphOutput};
use alloc::{boxed::Box, format, vec, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::ReaderOptions;

/// Raw graph bytes captured from a successful [`Reader`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// The graph is not parsed or validated.
pub type ReaderResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [reader] that imports input data into an RDF dataset.
///
/// This is a program-pattern wrapper, not an implementation of an I/O reader
/// trait. Input parsing and conversion are performed by the external program.
/// Execution uses the buffering and stream-handling behavior described in
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

    /// Sends the remaining input bytes to a new child and returns captured graph bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the reader exits unsuccessfully.
    pub async fn execute(&mut self) -> ReaderResult {
        let stdout = self.executor.execute_with_input(&mut self.input).await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Reader<Cursor<Vec<u8>>, ExecutorError> for Reader {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Reader {
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
