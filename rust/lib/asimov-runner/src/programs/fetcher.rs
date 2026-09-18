// This is free and unencumbered software released into the public domain.

//! URL protocol access through an external fetcher that produces RDF.

use crate::{CommandExt, Executor, ExecutorError, GraphOutput, JsonlStream};
use alloc::{
    boxed::Box,
    string::{String, ToString},
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::FetcherOptions;

/// A live JSONL graph stream, or an error starting the fetcher.
pub type FetcherResult = Result<JsonlStream, ExecutorError>;

/// An external [fetcher] that acts as a URL protocol client and produces RDF.
///
/// The input URL is passed as one command-line argument, and stdin is connected
/// to the null device. The external program handles the URL's protocol and
/// performs retrieval. Execution uses the concurrent streaming
/// behavior described in [`crate::programs`].
///
/// [fetcher]: https://asimov-specs.github.io/program-patterns/#fetcher
#[allow(unused)]
#[derive(Debug)]
pub struct Fetcher {
    executor: Executor,
    options: FetcherOptions,
    input: String,
    output: GraphOutput,
}

impl Fetcher {
    /// Configures a fetcher for the URL `input` without starting it.
    ///
    /// Adds `--output=<format>` when `options.output` is set, then
    /// `options.other`, then the URL as a single argument. The URL is copied
    /// without validation. `output` selects stdout handling; stderr is captured
    /// for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: FetcherOptions,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .option("output", options.output.as_ref())
            .args(&options.other)
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    /// Starts a new fetcher process and returns its live JSONL graph stream.
    ///
    /// # Errors
    ///
    /// Spawn failures are returned directly; output, wait, and exit failures are stream
    /// items. Consume the stream to completion to check process success.
    pub async fn execute(&mut self) -> FetcherResult {
        self.executor
            .execute_jsonl_with_output(&mut self.output)
            .await
    }
}

impl asimov_patterns::Fetcher<JsonlStream> for Fetcher {}

crate::pipeline::stage!(
    Fetcher,
    value,
    crate::Input::Ignored,
    value.output,
    None,
    value.options.output.as_deref()
);

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Fetcher {
    type Error = ExecutorError;

    async fn execute(&mut self) -> FetcherResult {
        self.execute().await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use futures_lite::StreamExt;

    #[tokio::test]
    async fn test_execute() {
        let mut fetcher = Fetcher::new(
            "/bin/sh",
            "printf '{}\\n'",
            GraphOutput::Ignored,
            FetcherOptions::builder().other("-c").build(),
        );
        let mut stream = fetcher.execute().await.unwrap();
        assert!(stream.next().await.is_none());
    }
}
