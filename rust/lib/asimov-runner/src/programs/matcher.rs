// This is free and unencumbered software released into the public domain.

//! Exact or approximate RDF matching through an external matcher program.

use crate::{Executor, ExecutorError, GraphInput, GraphOutput};
use alloc::{boxed::Box, format, vec, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::MatcherOptions;

/// Raw graph bytes captured from a successful [`Matcher`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// Matches are not parsed or validated.
pub type MatcherResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [matcher] that performs exact or approximate matching on RDF.
///
/// Output is RDF describing the matches, rather than necessarily a subset of
/// the input dataset. Matching rules belong to the external program;
/// this wrapper passes input bytes and command-line options through. Execution
/// uses the buffering and stream-handling behavior described in [`crate::programs`].
///
/// [matcher]: https://asimov-specs.github.io/program-patterns/#matcher
#[allow(unused)]
#[derive(Debug)]
pub struct Matcher {
    executor: Executor,
    options: MatcherOptions,
    input: GraphInput,
    output: GraphOutput,
}

impl Matcher {
    /// Configures a matcher without starting it.
    ///
    /// Adds any configured `--input=<format>` and `--output=<format>` arguments,
    /// followed by `options.other`. The input and output values select stdin
    /// and stdout; stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: GraphInput,
        output: GraphOutput,
        options: MatcherOptions,
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

    /// Sends the remaining graph input to a new child and returns captured matches.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the matcher exits unsuccessfully.
    pub async fn execute(&mut self) -> MatcherResult {
        let stdout = self.executor.execute_with_input(&mut self.input).await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Matcher<Cursor<Vec<u8>>, ExecutorError> for Matcher {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Matcher {
    async fn execute(&mut self) -> MatcherResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_options() {
        let options = MatcherOptions::builder()
            .input("jsonl")
            .output("nquads")
            .other("--exact")
            .maybe_other(Some("--custom"))
            .maybe_other(None::<&str>)
            .build();
        let mut matcher = Matcher::new(
            "asimov-test-matcher",
            GraphInput::Ignored,
            GraphOutput::Captured,
            options,
        );
        let args: Vec<_> = matcher.executor.command().as_std().get_args().collect();
        assert_eq!(
            args,
            ["--input=jsonl", "--output=nquads", "--exact", "--custom"]
        );
    }

    #[test]
    fn test_default_options() {
        let mut matcher = Matcher::new(
            "asimov-test-matcher",
            GraphInput::Ignored,
            GraphOutput::Captured,
            MatcherOptions::default(),
        );
        assert_eq!(matcher.executor.command().as_std().get_args().count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_execute() {
        let graph = b"{\"subject\":\"https://example.com/\"}\n";
        let mut matcher = Matcher::new(
            "cat",
            GraphInput::AsyncRead(Box::new(Cursor::new(graph.to_vec()))),
            GraphOutput::Captured,
            MatcherOptions::default(),
        );
        let output = asimov_patterns::Execute::execute(&mut matcher)
            .await
            .unwrap();
        assert_eq!(output.into_inner(), graph);
    }
}
