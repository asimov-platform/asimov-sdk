// This is free and unencumbered software released into the public domain.

//! RDF dataset export through an external writer program.

use crate::{AnyOutput, Executor, ExecutorError, GraphInput};
use alloc::{boxed::Box, format, vec, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::WriterOptions;

/// Raw serialized bytes captured from a successful [`Writer`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// The output is not decoded or validated.
pub type WriterResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [writer] that exports an RDF dataset to another representation.
///
/// This is a program-pattern wrapper, not an implementation of an I/O writer
/// trait. The external program interprets JSONL graph lines and performs serialization.
/// Execution uses the buffering and stream-handling behavior described in
/// [`crate::programs`].
///
/// [writer]: https://asimov-specs.github.io/program-patterns/#writer
#[allow(unused)]
#[derive(Debug)]
pub struct Writer {
    executor: Executor,
    options: WriterOptions,
    input: GraphInput,
    output: AnyOutput,
}

impl Writer {
    /// Configures a writer without starting it.
    ///
    /// Adds any configured `--input=<format>` and `--output=<format>` arguments,
    /// followed by `options.other`. The input and output values select stdin
    /// and stdout; stderr is captured for failure diagnostics.
    /// Byte input is lazily adapted into JSONL lines using [`GraphInput::into_jsonl`].
    pub fn new(
        program: impl AsRef<OsStr>,
        input: GraphInput,
        output: AnyOutput,
        options: WriterOptions,
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
            input: input.into_jsonl(),
            output,
        }
    }

    /// Sends the remaining graph input to a new child and returns captured serialized bytes.
    ///
    /// Input is fed concurrently with draining stdout and stderr. Output is
    /// captured until completion, or forwarded incrementally for `AsyncWrite`.
    /// Early child completion with unfinished input fails execution; see
    /// [`Executor::execute_with_input`].
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the writer exits unsuccessfully.
    pub async fn execute(&mut self) -> WriterResult {
        let stdout = self
            .executor
            .execute_with_io(&mut self.input, &mut self.output)
            .await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Writer<Cursor<Vec<u8>>> for Writer {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>> for Writer {
    type Error = ExecutorError;

    async fn execute(&mut self) -> WriterResult {
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
