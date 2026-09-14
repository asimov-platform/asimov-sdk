// This is free and unencumbered software released into the public domain.

//! RDF dataset entailment through an external reasoner program.

use crate::{Executor, ExecutorError, GraphInput, GraphOutput};
use alloc::{boxed::Box, format, vec, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::ReasonerOptions;

/// Raw graph bytes captured from a successful [`Reasoner`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// Inferred data is not parsed or validated.
pub type ReasonerResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [reasoner] that consumes an RDF dataset and emits entailed RDF.
///
/// Inference rules and the relationship between input and output graphs are
/// determined by the external program. This wrapper transports bytes using the
/// buffering and stream-handling behavior described in [`crate::programs`].
///
/// [reasoner]: https://asimov-specs.github.io/program-patterns/#reasoner
#[allow(unused)]
#[derive(Debug)]
pub struct Reasoner {
    executor: Executor,
    options: ReasonerOptions,
    input: GraphInput,
    output: GraphOutput,
}

impl Reasoner {
    /// Configures a reasoner without starting it.
    ///
    /// Adds any configured `--input=<format>` and `--output=<format>` arguments,
    /// followed by `options.other`. The input and output values select stdin
    /// and stdout; stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: GraphInput,
        output: GraphOutput,
        options: ReasonerOptions,
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

    /// Sends the remaining graph input to a new child and returns captured inference bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the reasoner exits unsuccessfully.
    pub async fn execute(&mut self) -> ReasonerResult {
        let stdout = self.executor.execute_with_input(&mut self.input).await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Reasoner<Cursor<Vec<u8>>, ExecutorError> for Reasoner {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Reasoner {
    async fn execute(&mut self) -> ReasonerResult {
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
