// This is free and unencumbered software released into the public domain.

//! URL-based directory iteration through an external lister program.

use crate::{Executor, ExecutorError, GraphOutput};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use async_trait::async_trait;
use core::pin::Pin;
use derive_more::Debug;
use futures_lite::Stream;
use std::{ffi::OsStr, process::Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};

pub use asimov_patterns::ListerOptions;

/// A live stream of JSONL lines from a [`Lister`].
///
/// Lines retain their LF or CRLF terminators; a final unterminated line is also
/// yielded. Bytes are neither decoded nor validated as JSON. Read, wait, and
/// unsuccessful-exit errors are yielded as a final error item. Consume the stream
/// to completion to check process success, even when stdout is not captured.
/// Dropping the stream before completion requests termination of the child.
pub type ListerStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, ExecutorError>> + Send>>;

/// A running listing stream, or an error starting the process.
pub type ListerResult = Result<ListerStream, ExecutorError>;

/// An external [lister] that iterates a directory URL and emits RDF for its entries.
///
/// The input URL is passed as one command-line argument, and stdin is connected
/// to the null device. Sorting and pagination are delegated to the external
/// program via options. Captured stdout is streamed one line at a time with
/// backpressure rather than buffered until the program exits. Stderr is drained
/// concurrently while reading and retained for failure diagnostics.
///
/// [lister]: https://asimov-specs.github.io/program-patterns/#lister
#[allow(unused)]
#[derive(Debug)]
pub struct Lister {
    executor: Executor,
    options: ListerOptions,
    input: String,
    output: GraphOutput,
}

impl Lister {
    /// Configures a lister for the directory URL `input` without starting it.
    ///
    /// Adds any configured `--sort`, `--offset`, `--limit`, and `--output`
    /// options as `--name=value` arguments, in that order, followed by
    /// `options.other` and the unvalidated URL. `output` selects stdout handling;
    /// stderr is captured for failure diagnostics.
    ///
    /// The specification requires `--limit` and `--output` support and defines
    /// `--sort` and `--offset` as optional capabilities. When supported, sorting
    /// precedes offset, and limit applies last. Counts refer to complete entries,
    /// not RDF statements. The SDK formats sort keys using `SortKeys`; the
    /// resulting expression must be supported by the selected program's profile.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: ListerOptions,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(ref sort) = options.sort {
                vec![format!("--sort={}", sort.to_string())]
            } else {
                vec![]
            })
            .args(if let Some(offset) = options.offset {
                vec![format!("--offset={}", offset)]
            } else {
                vec![]
            })
            .args(if let Some(limit) = options.limit {
                vec![format!("--limit={}", limit)]
            } else {
                vec![]
            })
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

    /// Starts a new lister process and returns its live listing stream.
    ///
    /// Returns after spawning, without waiting for output or process completion.
    /// With captured stdout, each item contains one line, including its terminator.
    /// Otherwise no lines are yielded, but the stream still checks the exit status.
    /// Stderr is buffered without a size bound; stdout buffers only the current
    /// line and a fixed-size read buffer. Use JSONL output for entry-wise streaming.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning fails. Subsequent I/O and exit
    /// errors are delivered through the stream, after any preceding output lines.
    pub async fn execute(&mut self) -> ListerResult {
        let mut process = self.executor.spawn().await?;
        let stdout = process.stdout.take();
        Ok(Box::pin(async_stream::try_stream! {
            // With stdout removed, this future drains stderr and waits for exit.
            // Keeping it in the stream also preserves the child's kill-on-drop policy.
            let completion = process.wait_with_output();
            tokio::pin!(completion);
            let mut output = None;

            if let Some(stdout) = stdout {
                let mut reader = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    let count = tokio::select! {
                        result = &mut completion, if output.is_none() => {
                            output = Some(result);
                            continue;
                        },
                        result = reader.read_until(b'\n', &mut line) => result,
                    }?;
                    // A cancelled read_until can leave a partial line in the
                    // buffer when process completion wins the select above.
                    if count == 0 && line.is_empty() {
                        break;
                    }
                    yield core::mem::take(&mut line);
                }
            }

            let output = match output {
                Some(output) => output?,
                None => completion.await?,
            };
            if !output.status.success() {
                Err(ExecutorError::from(output))?;
            }
        }))
    }
}

impl asimov_patterns::Lister<ListerStream, ExecutorError> for Lister {}

#[async_trait]
impl asimov_patterns::Execute<ListerStream, ExecutorError> for Lister {
    async fn execute(&mut self) -> ListerResult {
        self.execute().await
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use futures_lite::StreamExt;
    use std::time::Duration;
    use tokio::time::timeout;

    fn shell(script: &str, output: GraphOutput) -> Lister {
        Lister::new(
            "/bin/sh",
            script,
            output,
            ListerOptions {
                other: vec!["-c".into()],
                ..Default::default()
            },
        )
    }

    #[tokio::test]
    async fn streams_before_process_exit() {
        let mut stream = timeout(
            Duration::from_secs(5),
            shell("printf '{}\\n'; exec sleep 30", GraphOutput::Captured).execute(),
        )
        .await
        .expect("execute must return before the process exits")
        .unwrap();
        let line = timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("the first line must arrive before the process exits");
        assert_eq!(line.unwrap().unwrap(), b"{}\n");
        // Dropping the stream terminates the still-running child.
    }

    #[tokio::test]
    async fn preserves_line_bytes_and_unterminated_tail() {
        let mut stream = shell(
            "printf '{}\\n\\r\\n\\377\\n{\"last\":true}'",
            GraphOutput::Captured,
        )
        .execute()
        .await
        .unwrap();
        for expected in [b"{}\n".as_slice(), b"\r\n", b"\xff\n", b"{\"last\":true}"] {
            assert_eq!(stream.next().await.unwrap().unwrap(), expected);
        }
        assert!(stream.next().await.is_none());
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn drains_stderr_and_reports_failure_after_lines() {
        timeout(Duration::from_secs(5), async {
            let mut stream = shell(
                "i=0; while [ $i -lt 10000 ]; do printf 'diagnostic\\n' >&2; i=$((i + 1)); done; printf '{}\\n'; exit 1",
                GraphOutput::Captured,
            )
            .execute()
            .await
            .unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\n");
            match stream.next().await.unwrap().unwrap_err() {
                ExecutorError::UnexpectedFailure(Some(1), Some(stderr)) => {
                    assert_eq!(stderr, "diagnostic\n".repeat(10000));
                },
                error => panic!("unexpected error: {error}"),
            }
            assert!(stream.next().await.is_none());
        })
        .await
        .expect("stderr must be drained concurrently to avoid deadlock");
    }

    #[tokio::test]
    async fn uncaptured_stdout_still_checks_exit_status() {
        let mut stream = shell("printf '{}\\n'", GraphOutput::Ignored)
            .execute()
            .await
            .unwrap();
        assert!(stream.next().await.is_none());

        let mut stream = shell("exit 1", GraphOutput::Ignored)
            .execute()
            .await
            .unwrap();
        assert!(matches!(
            stream.next().await,
            Some(Err(ExecutorError::UnexpectedFailure(Some(1), _)))
        ));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn reports_spawn_errors_from_execute() {
        let result = Lister::new(
            "/this-lister-does-not-exist",
            "example:",
            GraphOutput::Captured,
            Default::default(),
        )
        .execute()
        .await;
        assert!(matches!(result, Err(ExecutorError::MissingProgram(_))));
    }
}
