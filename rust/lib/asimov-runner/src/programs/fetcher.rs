// This is free and unencumbered software released into the public domain.

//! URL protocol access through an external fetcher that produces RDF.

use crate::{Executor, ExecutorError, GraphOutput};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::FetcherOptions;

/// Raw graph bytes captured from a successful [`Fetcher`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// The graph is not parsed or validated.
pub type FetcherResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [fetcher] that acts as a URL protocol client and produces RDF.
///
/// The input URL is passed as one command-line argument, and stdin is connected
/// to the null device. The external program handles the URL's protocol and
/// performs retrieval. Execution uses the buffering and stream-handling
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
            .args(if let Some(ref output) = options.output {
                vec![format!("--output={}", output)]
            } else {
                vec![]
            })
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

    /// Runs a new fetcher process for the stored URL and returns captured RDF bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning or waiting fails, or if the
    /// fetcher exits unsuccessfully.
    pub async fn execute(&mut self) -> FetcherResult {
        let stdout = self.executor.execute().await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Fetcher<Cursor<Vec<u8>>, ExecutorError> for Fetcher {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Fetcher {
    async fn execute(&mut self) -> FetcherResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    //use asimov_patterns::Execute;

    #[tokio::test]
    async fn test_execute() {
        let mut fetcher = Fetcher::new(
            "curl",
            "https://www.google.com/robots.txt",
            GraphOutput::Ignored,
            FetcherOptions::default(),
        );
        let result = fetcher.execute().await;
        assert!(result.is_ok());
    }
}
