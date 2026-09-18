// This is free and unencumbered software released into the public domain.

//! SPARQL-to-RDF execution through an external dataset proxy.

use crate::{CommandExt, Executor, ExecutorError, GraphOutput, JsonlStream, QueryInput};
use alloc::boxed::Box;
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::AdapterOptions;

/// A live JSONL graph stream, or an error starting the adapter.
pub type AdapterResult = Result<JsonlStream, ExecutorError>;

/// An external [adapter] that proxies an RDF dataset using SPARQL queries.
///
/// The SPARQL query is passed to stdin as bytes, relying on the pattern's default
/// query-file argument of `-`. The external program evaluates the query and
/// emits RDF as JSONL lines. Execution uses the concurrent streaming and
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
            .option("output", options.output.as_ref())
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
    /// feeds it concurrently when polled. Subsequent executions have no query input.
    ///
    /// # Errors
    ///
    /// Spawn failures are returned directly; input, output, wait, and exit failures are
    /// stream items. Consume the stream to completion to check process success.
    pub async fn execute(&mut self) -> AdapterResult {
        self.executor
            .execute_jsonl_with_io(&mut self.input, &mut self.output)
            .await
    }
}

impl asimov_patterns::Adapter<JsonlStream> for Adapter {}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Adapter {
    type Error = ExecutorError;

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
