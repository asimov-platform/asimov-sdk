// This is free and unencumbered software released into the public domain.

//! Exact or approximate RDF matching through an external matcher program.

use crate::{CommandExt, Executor, ExecutorError, GraphInput, GraphOutput, JsonlStream};
use alloc::boxed::Box;
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::MatcherOptions;

/// A live JSONL graph stream, or an error starting the matcher.
pub type MatcherResult = Result<JsonlStream, ExecutorError>;

/// An external [matcher] that performs exact or approximate matching on RDF.
///
/// Output is RDF describing the matches, rather than necessarily a subset of
/// the input dataset. Matching rules belong to the external program;
/// this wrapper passes JSONL lines and command-line options through. Execution
/// uses the concurrent streaming behavior described in [`crate::programs`].
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
    /// Byte input is lazily adapted into JSONL batches using [`GraphInput::into_jsonl`].
    pub fn new(
        program: impl AsRef<OsStr>,
        input: GraphInput,
        output: GraphOutput,
        options: MatcherOptions,
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

    /// Starts a child and returns its live JSONL match stream.
    ///
    /// After successful spawning, input ownership moves into the stream, which
    /// feeds it concurrently when polled. Subsequent executions have no graph input.
    ///
    /// # Errors
    ///
    /// Spawn failures are returned directly; input, output, wait, and exit failures are
    /// stream items. Consume the stream to completion to check process success.
    pub async fn execute(&mut self) -> MatcherResult {
        self.executor
            .execute_jsonl_with_io(&mut self.input, &mut self.output)
            .await
    }
}

impl asimov_patterns::Matcher<JsonlStream> for Matcher {}

crate::batch::with_batching!(Matcher);

crate::pipeline::stage!(
    Matcher,
    value,
    value.input,
    value.output,
    value.options.input.as_deref(),
    value.options.output.as_deref()
);

#[async_trait]
impl asimov_patterns::Execute<JsonlStream> for Matcher {
    type Error = ExecutorError;

    async fn execute(&mut self) -> MatcherResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StreamExt;
    use alloc::vec::Vec;
    use std::io::Cursor;

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
        let mut output = asimov_patterns::Execute::execute(&mut matcher)
            .await
            .unwrap();
        let batch = output.next().await.unwrap().unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.lines().next().unwrap(), graph);
        assert!(output.next().await.is_none());
    }
}
