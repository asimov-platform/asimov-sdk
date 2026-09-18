// This is free and unencumbered software released into the public domain.

//! RDF dataset entailment through an external reasoner program.

use crate::{CommandExt, Executor, ExecutorError, GraphInput, GraphOutput, JsonlStream};
use alloc::boxed::Box;
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::ReasonerOptions;

/// A live JSONL graph stream, or an error starting the reasoner.
pub type ReasonerResult = Result<JsonlStream, ExecutorError>;

/// An external [reasoner] that consumes an RDF dataset and emits entailed RDF.
///
/// Inference rules and the relationship between input and output graphs are
/// determined by the external program. This wrapper transports JSONL lines using
/// the concurrent streaming behavior described in [`crate::programs`].
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
    /// Byte input is lazily adapted into JSONL lines using [`GraphInput::into_jsonl`].
    pub fn new(
        program: impl AsRef<OsStr>,
        input: GraphInput,
        output: GraphOutput,
        options: ReasonerOptions,
    ) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
            .option("input", options.input.as_ref())
            .option("output", options.output.as_ref())
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

    /// Starts a child and returns its live JSONL inference stream.
    ///
    /// After successful spawning, input ownership moves into the stream, which
    /// feeds it concurrently when polled. Subsequent executions have no graph input.
    ///
    /// # Errors
    ///
    /// Spawn failures are returned directly; input, output, wait, and exit failures are
    /// stream items. Consume the stream to completion to check process success.
    pub async fn execute(&mut self) -> ReasonerResult {
        self.executor
            .execute_jsonl_with_io(&mut self.input, &mut self.output)
            .await
    }
}

impl asimov_patterns::Reasoner<JsonlStream> for Reasoner {}

crate::pipeline::stage!(
    Reasoner,
    value,
    value.input,
    value.output,
    value.options.input.as_deref(),
    value.options.output.as_deref()
);

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Reasoner {
    type Error = ExecutorError;

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
