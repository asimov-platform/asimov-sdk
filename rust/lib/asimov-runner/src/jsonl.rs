// This is free and unencumbered software released into the public domain.

//! Line-based JSONL transport for graph programs.
//!
//! # Connecting graph programs
//!
//! `GraphInput::Jsonl` below connects user-space line streams. For a supervised
//! process chain using native OS pipes, use [`crate::Pipeline`].
//!
//! ```no_run
//! use asimov_runner::{Fetcher, GraphInput, GraphOutput, Matcher};
//! use futures_lite::StreamExt;
//!
//! # async fn example() -> Result<(), asimov_runner::ExecutorError> {
//! let source = Fetcher::new(
//!     "asimov-example-fetcher",
//!     "https://example.com/resource",
//!     GraphOutput::Captured,
//!     Default::default(),
//! ).execute().await?;
//! let mut matches = Matcher::new(
//!     "asimov-example-matcher",
//!     GraphInput::Jsonl(source),
//!     GraphOutput::Captured,
//!     Default::default(),
//! ).execute().await?;
//! while let Some(line) = matches.next().await {
//!     let bytes = line?;
//!     // Process this line; continue to EOF to observe eventual failures.
//! }
//! # Ok(())
//! # }
//! ```

use crate::{Executor, ExecutorError, Input, Output};
use alloc::{boxed::Box, vec::Vec};
use core::pin::Pin;
use futures_lite::{Stream, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// A fallible stream of JSONL lines, without JSON parsing or UTF-8 validation.
///
/// This alias is also usable for reader adapters and caller-supplied streams;
/// the type alone imposes no process lifecycle or record-validation behavior.
///
/// Streams returned by [`Executor::execute_jsonl`] and
/// [`Executor::execute_jsonl_with_input`] retain output LF/CRLF terminators and
/// a final unterminated line. Execution returns after spawning; polling drives
/// input, stdout, and stderr concurrently, with backpressure. No background task
/// drains the pipes while the stream is idle. Consume to completion to check
/// exit status: errors are final items, following any lines already yielded.
/// Stderr is buffered without a size bound; stdout buffers the current line and
/// a fixed-size read buffer, with no maximum line length. Dropping a process
/// stream requests termination under the executor's default kill-on-drop policy;
/// overriding that policy through [`Executor::command`] also affects streaming.
/// [`Executor::execute_jsonl_with_io`] additionally supports forwarding instead
/// of capture: those streams yield no payload lines but must still be consumed
/// to drive I/O and observe completion. Errors use
/// [`crate::ExecutionCompletion::into_result`] precedence.
pub type JsonlStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, ExecutorError>> + Send>>;

/// Splits an asynchronous byte reader into lines, preserving all bytes.
///
/// Splits at LF, retaining LF/CRLF endings and a final unterminated line. Blank
/// lines are yielded and neither JSON nor UTF-8 is validated. Reading is driven
/// by polling, with no maximum line length. A read error is yielded once and
/// ends the stream; bytes in a partially read line are not yielded on error.
/// Dropping this stream drops its reader, without checking any process status.
pub fn jsonl_lines(reader: impl AsyncRead + Send + Unpin + 'static) -> JsonlStream {
    Box::pin(async_stream::try_stream! {
        let mut reader = BufReader::new(reader);
        loop {
            let mut line = Vec::new();
            if reader.read_until(b'\n', &mut line).await? == 0 {
                break;
            }
            yield line;
        }
    })
}

impl Executor {
    /// Spawns a program and streams its stdout as JSONL lines.
    ///
    /// Uses the configured standard streams; stdout must be piped to yield lines.
    /// No input is written. Any piped stdin is closed when the stream is polled.
    /// Even with ignored or inherited stdout, consume the stream to completion
    /// to check process success. See [`JsonlStream`] for polling and drop behavior.
    ///
    /// # Errors
    ///
    /// Spawn errors are returned directly; read, wait, and exit errors are stream items.
    pub async fn execute_jsonl(&mut self) -> Result<JsonlStream, ExecutorError> {
        self.execute_jsonl_with_input(&mut Input::Ignored).await
    }

    /// Spawns a program, feeding input concurrently with streaming its stdout.
    ///
    /// After a successful spawn, ownership of `input` moves into the returned
    /// stream and it is replaced with [`Input::Ignored`]. Repeated execution does
    /// not replay input. No input is consumed on spawn failure. Configure the
    /// command's stdin with [`Input::as_stdio`] before calling this method.
    /// Stdout and stderr handling also use the existing command configuration.
    /// [`Input::AsyncRead`] copies raw bytes; [`Input::Jsonl`] writes framed lines.
    /// This method does not automatically adapt byte input into JSONL.
    ///
    /// Polling drives I/O; see [`JsonlStream`] for buffering and drop behavior.
    /// Early child completion cancels the input feed and reports
    /// [`ExecutorError::IncompleteInput`] if the child otherwise succeeded.
    /// Errors follow [`crate::ExecutionCompletion::into_result`] precedence.
    ///
    /// # Errors
    ///
    /// Spawn errors are returned directly; input, read, wait, and exit errors are
    /// stream items. Non-ignored input requires piped stdin; otherwise feeding
    /// it reports an I/O `InvalidInput` error through the stream.
    pub async fn execute_jsonl_with_input(
        &mut self,
        input: &mut Input,
    ) -> Result<JsonlStream, ExecutorError> {
        self.execute_jsonl_with_io(input, &mut Output::Captured)
            .await
    }

    /// Spawns a graph producer with the supplied stdout policy and no input.
    /// Configure stdout with [`Output::as_stdio`] first. Forwarded output is
    /// written while the returned stream is polled and yields no payload items.
    /// Spawn errors are returned directly; subsequent failures are stream items.
    pub async fn execute_jsonl_with_output(
        &mut self,
        output: &mut Output,
    ) -> Result<JsonlStream, ExecutorError> {
        self.execute_jsonl_with_io(&mut Input::Ignored, output)
            .await
    }

    /// Spawns a graph program with concurrent input and stdout routing.
    ///
    /// Configure the command using the policies' `as_stdio` methods first.
    /// Successful spawning transfers input and any output writer into the stream.
    /// Captured stdout yields lines; other modes yield only eventual errors.
    /// A transferred writer is flushed at EOF, not shut down, and the wrapper's
    /// output policy becomes [`Output::Ignored`] for subsequent executions.
    /// Non-writer output policies remain reusable. Spawn failure consumes neither.
    ///
    /// # Errors
    ///
    /// Spawn errors are returned directly. Input, transport, writer, wait, and
    /// exit failures are stream items. Success requires complete input delivery
    /// and zero exit status; see [`crate::ExecutionCompletion::into_result`].
    pub async fn execute_jsonl_with_io(
        &mut self,
        input: &mut Input,
        output: &mut Output,
    ) -> Result<JsonlStream, ExecutorError> {
        let mut process = self.spawn().await?;
        let stdout = if matches!(output, Output::Captured) {
            process.stdout.take()
        } else {
            None
        };
        let mut input = core::mem::replace(input, Input::Ignored);
        let mut destination = output.take_for_stream();
        Ok(Box::pin(async_stream::try_stream! {
            let completion = async move {
                crate::executor::communicate(process, &mut input, &mut destination).await
            };
            tokio::pin!(completion);
            let mut output = None;
            if let Some(stdout) = stdout {
                let mut lines = jsonl_lines(stdout);
                loop {
                    let line = tokio::select! {
                        result = &mut completion, if output.is_none() => {
                            output = Some(result);
                            continue;
                        },
                        line = lines.next() => line,
                    };
                    match line {
                        Some(line) => yield line?,
                        None => break,
                    }
                }
            }
            let output = match output {
                Some(output) => output?,
                None => completion.await?,
            };
            output.into_result()?;
        }))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::*;
    use alloc::{string::ToString, vec};
    use std::{io::Cursor, process::Stdio, time::Duration};
    use tokio::time::timeout;

    #[tokio::test]
    async fn graph_producers_stream_before_exit() {
        let script = "printf '{}\\n'; exec sleep 30";
        let args = vec!["-c".to_string(), script.to_string()];
        macro_rules! check {
            ($runner:expr) => {{
                let mut stream = timeout(Duration::from_secs(5), $runner.execute())
                    .await
                    .expect("spawn must not wait for exit")
                    .unwrap();
                let line = timeout(Duration::from_secs(5), stream.next())
                    .await
                    .expect("output must not wait for exit");
                assert_eq!(line.unwrap().unwrap(), b"{}\n");
            }};
        }
        check!(Adapter::new(
            "/bin/sh",
            Input::Ignored,
            GraphOutput::Captured,
            AdapterOptions {
                other: args.clone(),
                ..Default::default()
            },
        ));
        check!(Emitter::new(
            "/bin/sh",
            GraphOutput::Captured,
            EmitterOptions {
                other: args.clone(),
                ..Default::default()
            },
        ));
        check!(Fetcher::new(
            "/bin/sh",
            script,
            GraphOutput::Captured,
            FetcherOptions {
                other: vec!["-c".into()],
                ..Default::default()
            },
        ));
        check!(Reader::new(
            "/bin/sh",
            Input::Ignored,
            GraphOutput::Captured,
            ReaderOptions {
                other: args.clone(),
                ..Default::default()
            },
        ));
        check!(Matcher::new(
            "/bin/sh",
            Input::Ignored,
            GraphOutput::Captured,
            MatcherOptions {
                other: args.clone(),
                ..Default::default()
            },
        ));
        check!(Reasoner::new(
            "/bin/sh",
            Input::Ignored,
            GraphOutput::Captured,
            ReasonerOptions {
                other: args,
                ..Default::default()
            },
        ));
    }

    #[tokio::test]
    async fn output_is_available_while_input_is_still_open() {
        timeout(Duration::from_secs(5), async {
            let source = Box::pin(async_stream::try_stream! {
                yield b"{}".to_vec();
                core::future::pending::<()>().await;
            });
            let mut stream = Matcher::new(
                "/bin/cat",
                GraphInput::Jsonl(source),
                GraphOutput::Captured,
                MatcherOptions::default(),
            )
            .execute()
            .await
            .unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\n");
        })
        .await
        .expect("output must not wait for input EOF");
    }

    #[tokio::test]
    async fn dropping_stream_terminates_child() {
        let mut stream = Emitter::new(
            "/bin/sh",
            GraphOutput::Captured,
            EmitterOptions::builder()
                .other("-c")
                .other("printf '%s\\n' $$; exec sleep 30")
                .build(),
        )
        .execute()
        .await
        .unwrap();
        let pid = timeout(Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let pid = std::str::from_utf8(&pid).unwrap().trim();
        drop(stream);
        timeout(Duration::from_secs(5), async {
            while tokio::process::Command::new("/bin/kill")
                .args(["-0", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .unwrap()
                .success()
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("dropping the stream must terminate the child");
    }

    #[tokio::test]
    async fn feeds_large_graph_while_draining_both_output_pipes() {
        timeout(Duration::from_secs(10), async {
            let line = [b"\"".as_slice(), &vec![b'x'; 1024], b"\"\n"].concat();
            let source_line = line.clone();
            let source = Box::pin(async_stream::try_stream! {
                for _ in 0..4096 {
                    yield source_line.clone();
                }
            });
            let mut stream = Reasoner::new(
                "/bin/sh",
                GraphInput::Jsonl(source),
                GraphOutput::Captured,
                ReasonerOptions::builder().other("-c").other(
                    "i=0; while [ $i -lt 10000 ]; do printf 'diagnostic\\n' >&2; i=$((i + 1)); done; cat",
                ).build(),
            ).execute().await.unwrap();
            let mut count = 0;
            while let Some(actual) = stream.next().await {
                assert_eq!(actual.unwrap(), line);
                count += 1;
            }
            assert_eq!(count, 4096);
        }).await.expect("full-duplex graph transport must not deadlock");
    }

    #[tokio::test]
    async fn composes_graph_producers_and_consumers() {
        let source = Emitter::new(
            "/bin/sh",
            GraphOutput::Captured,
            EmitterOptions::builder()
                .other("-c")
                .other("printf '{}\\r\\n{\"last\":true}'")
                .build(),
        )
        .execute()
        .await
        .unwrap();
        let mut stream = Matcher::new(
            "/bin/cat",
            GraphInput::Jsonl(source),
            GraphOutput::Captured,
            MatcherOptions::default(),
        )
        .execute()
        .await
        .unwrap();
        assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\r\n");
        assert_eq!(stream.next().await.unwrap().unwrap(), b"{\"last\":true}\n");
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn propagates_upstream_failure() {
        timeout(Duration::from_secs(5), async {
            let source = Emitter::new(
                "/bin/sh",
                GraphOutput::Captured,
                EmitterOptions::builder()
                    .other("-c")
                    .other("printf 'upstream failed' >&2; exit 65")
                    .build(),
            )
            .execute()
            .await
            .unwrap();
            let mut stream = Matcher::new(
                "/bin/cat",
                GraphInput::Jsonl(source),
                GraphOutput::Captured,
                MatcherOptions::default(),
            )
            .execute()
            .await
            .unwrap();
            match stream.next().await.unwrap().unwrap_err() {
                ExecutorError::Failure(code, Some(stderr)) => {
                    assert_eq!(code.code(), Some(65));
                    assert_eq!(stderr, "upstream failed");
                },
                error => panic!("unexpected error: {error}"),
            }
            assert!(stream.next().await.is_none());
        })
        .await
        .expect("source failure must terminate the downstream child");
    }

    #[tokio::test]
    async fn early_exit_cancels_idle_input() {
        timeout(Duration::from_secs(5), async {
            let input = Input::Jsonl(Box::pin(futures_lite::stream::pending()));
            let mut stream = Matcher::new(
                "/bin/sh",
                input,
                GraphOutput::Captured,
                MatcherOptions::builder()
                    .other("-c")
                    .other("exit 65")
                    .build(),
            )
            .execute()
            .await
            .unwrap();
            assert!(matches!(
                stream.next().await,
                Some(Err(ExecutorError::Failure(_, _)))
            ));
            assert!(stream.next().await.is_none());
        })
        .await
        .expect("child exit must not wait for an idle input stream");
    }

    #[tokio::test]
    async fn writer_and_indexer_consume_jsonl() {
        let input = || {
            GraphInput::Jsonl(Box::pin(futures_lite::stream::iter([
                Ok(b"{}".to_vec()),
                Ok(b"[]\r\n".to_vec()),
            ])))
        };
        let script = "test \"$(cat)\" = \"$(printf '{}\\n[]\\r')\" || exit 65";
        Indexer::new(
            "/bin/sh",
            input(),
            IndexerOptions::builder().other("-c").other(script).build(),
        )
        .execute()
        .await
        .unwrap();
        let output = Writer::new(
            "/bin/sh",
            input(),
            AnyOutput::Captured,
            WriterOptions::builder()
                .other("-c")
                .other(alloc::format!("{script}; printf '\\000\\377'"))
                .build(),
        )
        .execute()
        .await
        .unwrap();
        assert_eq!(output.into_inner(), b"\x00\xff");
    }

    #[tokio::test]
    async fn byte_graph_input_is_framed_and_spawn_failure_preserves_input() {
        let mut input = GraphInput::AsyncRead(Box::new(Cursor::new(b"{}".to_vec()))).into_jsonl();
        let mut missing = Executor::new("/this-jsonl-program-does-not-exist");
        missing.command().stdin(input.as_stdio());
        assert!(matches!(
            missing.execute_jsonl_with_input(&mut input).await,
            Err(ExecutorError::MissingProgram(_))
        ));
        let mut executor = Executor::new("/bin/cat");
        executor
            .command()
            .stdin(input.as_stdio())
            .stdout(Stdio::piped());
        let mut stream = executor.execute_jsonl_with_input(&mut input).await.unwrap();
        assert!(matches!(input, Input::Ignored));
        assert_eq!(stream.next().await.unwrap().unwrap(), b"{}\n");
        assert!(stream.next().await.is_none());
    }
}
