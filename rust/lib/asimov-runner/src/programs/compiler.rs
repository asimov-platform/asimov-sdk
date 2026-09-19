// This is free and unencumbered software released into the public domain.

//! Natural-language-to-SPARQL compilation through an external compiler program.

use crate::{Executor, ExecutorError, QueryOutput, TextInput};
use alloc::{boxed::Box, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::CompilerOptions;

/// Raw query bytes captured from a successful [`Compiler`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// Captured bytes, including whitespace and trailing newlines, are preserved.
/// The pattern requires UTF-8 SPARQL, but this wrapper does not decode or validate it.
pub type CompilerResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>;

/// An external [compiler] that translates natural-language text into a SPARQL query.
///
/// Input bytes are copied to stdin, relying on the pattern's default input-file
/// operand of `-`. The external program produces one UTF-8 SPARQL query for an
/// adapter, without Markdown fences or explanatory prose outside the query.
/// Dataset and vocabulary assumptions, and compatibility with the intended
/// adapter's supported query forms, belong to that program.
///
/// This wrapper captures the generated query without parsing or executing it.
/// Use [`QueryOutput::Captured`] to retrieve it for an [`Adapter`](crate::Adapter).
/// Input is consumed from its current position and is not rewound on subsequent
/// executions. Buffering, output routing, cancellation, and current stream-I/O
/// limitations follow [`crate::programs`].
///
/// # Example
///
/// Compile a request, then pass the captured query to a dataset adapter:
///
/// ```no_run
/// use asimov_runner::{
///     Adapter, AdapterOptions, Compiler, CompilerOptions, GraphOutput,
///     QueryInput, QueryOutput, StreamExt, TextInput,
/// };
/// use std::io::Cursor;
///
/// # async fn example() -> Result<(), asimov_runner::ExecutorError> {
/// let mut compiler = Compiler::new(
///     "asimov-example-compiler",
///     TextInput::AsyncRead(Box::new(Cursor::new(b"Describe the known cities.".to_vec()))),
///     QueryOutput::Captured,
///     CompilerOptions::default(),
/// );
/// let query = compiler.execute().await?;
///
/// let mut adapter = Adapter::new(
///     "asimov-example-adapter",
///     QueryInput::AsyncRead(Box::new(query)),
///     GraphOutput::Captured,
///     AdapterOptions::default(),
/// );
/// let mut graph = adapter.execute().await?;
/// while let Some(batch) = graph.next().await {
///     for bytes in batch?.lines() {
///         // Process this JSONL graph line.
///     }
/// }
/// # Ok(())
/// # }
/// ```
///
/// [compiler]: https://asimov-specs.github.io/program-patterns/#compiler
#[allow(unused)]
#[derive(Debug)]
pub struct Compiler {
    executor: Executor,
    options: CompilerOptions,
    input: TextInput,
    output: QueryOutput,
}

impl Compiler {
    /// Configures a compiler without starting it.
    ///
    /// Forwards `options.other` as individual arguments in order. The pattern
    /// defines no standard model or format flags, so none are generated.
    /// `input` and `output` select stdin and stdout handling; stderr is captured
    /// for failure diagnostics.
    ///
    /// To select a named input file, supply its path through `options.other`
    /// after any extension options and use [`TextInput::Ignored`]. No standard
    /// output-file operand is defined. This constructor does not validate
    /// operands, extension support, or the input's encoding.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: TextInput,
        output: QueryOutput,
        options: CompilerOptions,
    ) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
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

    /// Sends the remaining text input to a new child and returns captured query bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the compiler exits unsuccessfully. Query syntax and UTF-8 validity
    /// are the external program's responsibility and are not checked here.
    pub async fn execute(&mut self) -> CompilerResult {
        let stdout = self
            .executor
            .execute_with_io(&mut self.input, &mut self.output)
            .await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Compiler<Cursor<Vec<u8>>> for Compiler {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>> for Compiler {
    type Error = ExecutorError;

    async fn execute(&mut self) -> CompilerResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_invocation() {
        for options in [
            CompilerOptions::default(),
            CompilerOptions::builder().build(),
        ] {
            let mut compiler = Compiler::new(
                "asimov-test-compiler",
                TextInput::Ignored,
                QueryOutput::Captured,
                options,
            );
            // The standard compiler invocation has no implicit model or format flags.
            assert_eq!(compiler.executor.command().as_std().get_args().count(), 0);
        }
    }

    #[test]
    fn test_argument_boundaries_and_order() {
        let options = CompilerOptions::builder()
            .other("--dataset")
            .maybe_other(Some("value with spaces; $HOME"))
            .maybe_other(None::<&str>)
            .other("--")
            .other("-request with spaces.txt")
            .build();
        let mut compiler = Compiler::new(
            "asimov-test-compiler",
            TextInput::Ignored,
            QueryOutput::Captured,
            options,
        );
        let args: Vec<_> = compiler.executor.command().as_std().get_args().collect();
        assert_eq!(
            args,
            [
                "--dataset",
                "value with spaces; $HOME",
                "--",
                "-request with spaces.txt",
            ]
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_compile_and_pass_query_to_adapter() {
        use crate::{Adapter, AdapterOptions, GraphOutput, QueryInput, StreamExt};

        async fn compile(
            compiler: &mut impl asimov_patterns::Compiler<Cursor<Vec<u8>>, Error = ExecutorError>,
        ) -> CompilerResult {
            compiler.execute().await
        }

        // Local fixtures verify transport and composition without an inference provider.
        let options = CompilerOptions::builder()
            .other("-c")
            .other(
                "test \"$(cat)\" = 'Describe café locations.' || exit 65; \
                 printf '%s\n' 'PREFIX ex: <https://example.com/>' \
                 'CONSTRUCT { ?s ?p ?o } WHERE {' '  ?s ?p ?o' '}'",
            )
            .build();
        let mut compiler = Compiler::new(
            "/bin/sh",
            TextInput::AsyncRead(Box::new(Cursor::new(
                "Describe café locations.\n".as_bytes().to_vec(),
            ))),
            QueryOutput::Captured,
            options,
        );
        let query = compile(&mut compiler).await.unwrap();
        let expected =
            b"PREFIX ex: <https://example.com/>\nCONSTRUCT { ?s ?p ?o } WHERE {\n  ?s ?p ?o\n}\n";
        assert_eq!(query.position(), 0);
        assert_eq!(query.get_ref(), expected);

        // The adapter fixture echoes the query it receives, exposing any lost bytes.
        let mut adapter = Adapter::new(
            "/bin/cat",
            QueryInput::AsyncRead(Box::new(query)),
            GraphOutput::Captured,
            AdapterOptions::default(),
        );
        let mut stream = adapter.execute().await.unwrap();
        let mut output = Vec::new();
        while let Some(batch) = stream.next().await {
            for line in batch.unwrap().lines() {
                output.extend_from_slice(line);
            }
        }
        assert_eq!(output, expected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_ignored_output_is_not_captured() {
        let mut compiler = Compiler::new(
            "/bin/sh",
            TextInput::Ignored,
            QueryOutput::Ignored,
            CompilerOptions::builder()
                .other("-c")
                .other("printf '%s\n' 'CONSTRUCT {} WHERE {}'")
                .build(),
        );
        assert!(compiler.execute().await.unwrap().into_inner().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_failure_preserves_diagnostics_instead_of_returning_partial_query() {
        let mut compiler = Compiler::new(
            "/bin/sh",
            TextInput::Ignored,
            QueryOutput::Captured,
            CompilerOptions::builder()
                .other("-c")
                .other(
                    "printf '%s\n' 'CONSTRUCT {'; \
                     printf '%s\n' 'Unable to compile the request.' >&2; exit 65",
                )
                .build(),
        );
        match compiler.execute().await {
            Err(ExecutorError::Failure(error, Some(stderr))) => {
                assert_eq!(error.code(), Some(65));
                assert_eq!(stderr, "Unable to compile the request.\n");
            },
            result => panic!("expected a compilation failure with diagnostics, got {result:?}"),
        }
    }
}
