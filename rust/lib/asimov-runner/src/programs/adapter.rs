// This is free and unencumbered software released into the public domain.

//! SPARQL-to-RDF execution through an external dataset proxy.

use crate::{Executor, ExecutorError, GraphOutput, QueryInput};
use alloc::{boxed::Box, format, vec, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::AdapterOptions;

/// Raw graph bytes captured from a successful [`Adapter`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// The graph is not parsed or validated.
pub type AdapterResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [adapter] that proxies an RDF dataset using SPARQL queries.
///
/// The SPARQL query is passed to stdin as bytes, relying on the pattern's default
/// query-file argument of `-`. The external program evaluates the query and
/// emits RDF in the requested output format. Execution uses the buffering and
/// stream-handling behavior described in [`crate::programs`].
///
/// [adapter]: https://asimov-specs.github.io/program-patterns/#adapter
#[allow(unused)]
#[derive(Debug)]
pub struct Adapter {
    executor: Executor,
    options: AdapterOptions,
    input: QueryInput,
    output: GraphOutput,
}

impl Adapter {
    /// Configures an adapter without starting it.
    ///
    /// Adds `--output=<format>` when `options.output` is set, followed by
    /// `options.other`. The input and output values select stdin and stdout;
    /// stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: QueryInput,
        output: GraphOutput,
        options: AdapterOptions,
    ) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
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

    /// Sends the remaining query input to a new child and returns captured graph bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the adapter exits unsuccessfully.
    pub async fn execute(&mut self) -> AdapterResult {
        let stdout = self.executor.execute_with_input(&mut self.input).await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Adapter<Cursor<Vec<u8>>, ExecutorError> for Adapter {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Adapter {
    async fn execute(&mut self) -> AdapterResult {
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
