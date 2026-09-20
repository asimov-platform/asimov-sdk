// This is free and unencumbered software released into the public domain.

//! Batched JSONL transport for graph programs, with line framing underneath.
//!
//! # Connecting graph programs
//!
//! `GraphInput::Jsonl` below connects user-space batch streams. For a supervised
//! process chain using native OS pipes, use [`crate::Pipeline`].
//!
//! ```no_run
//! use asimov_runner::{Fetcher, GraphInput, GraphOutput, Matcher, StreamExt};
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
//! while let Some(batch) = matches.next().await {
//!     for bytes in batch?.lines() {
//!         // Process a line, or submit the whole batch to a network service.
//!     }
//!     // Continue to EOF to observe eventual failures.
//! }
//! # Ok(())
//! # }
//! ```

use crate::batch::{FrameStream, FramedLine, batch_frames};
use crate::{
    BatchOptions, BatchStream, Executor, ExecutorError, Input, LineStream, Output, StreamExt,
};
use alloc::boxed::Box;
use tokio::io::AsyncRead;

/// A fallible stream of [`crate::JsonlBatch`] values, without JSON parsing or UTF-8 validation.
///
/// This alias is also usable for reader adapters and caller-supplied streams;
/// the type alone imposes no process lifecycle or record-validation behavior.
///
/// Streams returned by [`Executor::execute_jsonl`] and
/// [`Executor::execute_jsonl_with_input`] retain output LF/CRLF terminators and
/// a final unterminated line. Execution returns after spawning; polling drives
/// input, stdout, and stderr concurrently, with backpressure. No background task
/// drains the pipes while the stream is idle. Consume to completion to check
/// exit status: buffered complete lines are flushed in a partial batch before a
/// subsequent error is yielded as the final item. [`BatchOptions`] bounds batch
/// count/size and collection delay. Stderr and individual lines have no configured
/// size bound. Dropping a process
/// stream requests termination under the executor's default kill-on-drop policy;
/// overriding that policy through [`Executor::command`] also affects streaming.
/// [`Executor::execute_jsonl_with_io`] additionally supports forwarding instead
/// of capture: those streams yield no payload batches but must still be consumed
/// to drive I/O and observe completion. Errors use
/// [`crate::ExecutionCompletion::into_result`] precedence.
pub type JsonlStream = BatchStream<ExecutorError>;

/// Frames an asynchronous byte reader into validated shared lines, preserving bytes.
///
/// Splits at LF, retaining LF/CRLF endings and a final unterminated line. Blank
/// lines are yielded and neither JSON nor UTF-8 is validated. Reading is driven
/// by polling, with no maximum line length. A read error is yielded once and
/// ends the stream; bytes in a partially read line are not yielded on error.
/// Dropping this stream drops its reader, without checking any process status.
/// Reads use shared backing buffers, not a separate payload allocation per line.
/// Retained lines can keep a larger read allocation alive; compact sparse,
/// long-lived selections using [`crate::JsonlLine::into_compact`] or [`crate::JsonlBatch::into_compact`].
pub fn jsonl_lines(reader: impl AsyncRead + Send + Unpin + 'static) -> LineStream {
    Box::pin(jsonl_frames(reader).map(|line| line.map(FramedLine::into_line)))
}

/// Internal line framing with enough read-buffer provenance to build contiguous
/// batches. Public individual lines contain only their sliced Bytes views.
pub(crate) fn jsonl_frames(reader: impl AsyncRead + Send + Unpin + 'static) -> FrameStream {
    Box::pin(asimov_flow::jsonl::jsonl_frames(reader).map(|line| line.map_err(Into::into)))
}

/// Reads and batches JSONL without changing bytes or line endings. Reading is
/// lazy; see [`crate::batch_lines`] for thresholds, timer, EOF, and error behavior.
/// The framer and batch builder retain contiguous backing metadata; extracting
/// individual `JsonlLine` values yields ordinary sliced `Bytes` views.
pub fn jsonl_batches(
    reader: impl AsyncRead + Send + Unpin + 'static,
    options: BatchOptions,
) -> JsonlStream {
    batch_frames(jsonl_frames(reader), options)
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use crate::{Bytes, JsonlBatch, JsonlLine};
    use alloc::{vec, vec::Vec};
    use std::io::Cursor;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn frames_a_read_buffer_into_shared_lines_without_payload_copies() {
        let mut stream = jsonl_lines(Cursor::new(b"a\nb\r\nc\nlast"));
        let mut lines = Vec::new();
        for expected in [b"a\n".as_slice(), b"b\r\n", b"c\n"] {
            let line = stream.next().await.unwrap().unwrap();
            assert!(matches!(line, JsonlLine::Shared(_)));
            assert_eq!(line.as_bytes(), expected);
            lines.push(line);
        }
        assert_eq!(
            lines[1].as_bytes().as_ptr(),
            lines[0].as_bytes().as_ptr().wrapping_add(2)
        );
        let batch = JsonlBatch::new(lines);
        // Detached Bytes views do not retain batch-level coalescing metadata.
        assert!(batch.as_contiguous_bytes().is_none());
        let last = stream.next().await.unwrap().unwrap();
        assert_eq!(last.as_bytes(), b"last");
        assert!(!last.is_terminated());
        assert!(stream.next().await.is_none());
        // Retained lines remain valid after both reader and framing state are gone.
        drop(stream);
        assert_eq!(
            batch.lines().collect::<Vec<_>>(),
            [b"a\n".as_slice(), b"b\r\n", b"c\n"]
        );
    }

    #[tokio::test]
    async fn reader_batches_preserve_contiguous_metadata_across_batch_boundaries() {
        let mut batches = jsonl_batches(
            Cursor::new(b"a\nb\nc\nd\ntail"),
            BatchOptions::new(2, 1024, core::time::Duration::from_secs(1)).unwrap(),
        );
        let first = batches.next().await.unwrap().unwrap();
        let second = batches.next().await.unwrap().unwrap();
        let tail = batches.next().await.unwrap().unwrap();
        assert!(batches.next().await.is_none());
        drop(batches);
        assert_eq!(first.as_contiguous_bytes(), Some(b"a\nb\n".as_slice()));
        assert_eq!(second.as_contiguous_bytes(), Some(b"c\nd\n".as_slice()));
        assert_eq!(
            second.as_contiguous_bytes().unwrap().as_ptr(),
            first
                .as_contiguous_bytes()
                .unwrap()
                .as_ptr()
                .wrapping_add(4)
        );
        let pointer = first.as_contiguous_bytes().unwrap().as_ptr();
        let lines = first.into_lines();
        assert_eq!(lines[0].as_bytes().as_ptr(), pointer);
        assert_eq!(lines[1].as_bytes().as_ptr(), pointer.wrapping_add(2));
        assert_eq!(tail.lines().collect::<Vec<_>>(), [b"tail".as_slice()]);
        assert!(tail.as_contiguous_bytes().is_none());
    }

    #[tokio::test]
    async fn byte_threshold_lookahead_preserves_batch_views() {
        let mut batches = jsonl_batches(
            Cursor::new(b"a\nb\nc\n"),
            BatchOptions::new(10, 3, core::time::Duration::from_secs(1)).unwrap(),
        );
        let first = batches.next().await.unwrap().unwrap();
        let second = batches.next().await.unwrap().unwrap();
        let third = batches.next().await.unwrap().unwrap();
        assert!(batches.next().await.is_none());
        assert_eq!(first.as_contiguous_bytes(), Some(b"a\n".as_slice()));
        assert_eq!(second.as_contiguous_bytes(), Some(b"b\n".as_slice()));
        assert_eq!(third.as_contiguous_bytes(), Some(b"c\n".as_slice()));
        let first_pointer = first.as_contiguous_bytes().unwrap().as_ptr();
        assert_eq!(
            second.as_contiguous_bytes().unwrap().as_ptr(),
            first_pointer.wrapping_add(2)
        );
        assert_eq!(
            third.as_contiguous_bytes().unwrap().as_ptr(),
            first_pointer.wrapping_add(4)
        );
    }

    #[tokio::test]
    async fn frames_split_crlf_long_lines_and_unterminated_tails() {
        let (reader, mut writer) = tokio::io::duplex(7);
        let mut expected = vec![b'x'; 128 * 1024];
        expected.extend_from_slice(b"\r\n");
        let data = expected.clone();
        let writing = tokio::spawn(async move {
            writer.write_all(&data[..data.len() - 1]).await.unwrap();
            tokio::task::yield_now().await;
            writer.write_all(b"\ntail").await.unwrap();
        });
        let mut lines = jsonl_lines(reader);
        assert_eq!(lines.next().await.unwrap().unwrap().as_bytes(), expected);
        assert_eq!(lines.next().await.unwrap().unwrap().as_bytes(), b"tail");
        assert!(lines.next().await.is_none());
        writing.await.unwrap();
    }

    #[tokio::test]
    async fn every_chunk_boundary_preserves_framing() {
        let data = Bytes::from_static(b"\n{}\r\n\xff\nend");
        for width in 1..=data.len() {
            let (reader, mut writer) = tokio::io::duplex(width);
            let source = data.clone();
            let writing = tokio::spawn(async move {
                for chunk in source.chunks(width) {
                    writer.write_all(chunk).await.unwrap();
                    tokio::task::yield_now().await;
                }
            });
            let mut lines = jsonl_lines(reader);
            for expected in [b"\n".as_slice(), b"{}\r\n", b"\xff\n", b"end"] {
                let line = lines.next().await.unwrap().unwrap();
                assert_eq!(line.as_bytes(), expected);
                assert!(JsonlLine::shared(line.into_bytes()).is_ok());
            }
            assert!(lines.next().await.is_none());
            writing.await.unwrap();
        }
    }
}

impl Executor {
    /// Spawns a program and streams its stdout as JSONL batches.
    ///
    /// Uses the configured standard streams; stdout must be piped to yield batches.
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
    /// Captured stdout yields batches using [`Self::batch_options`]; other modes
    /// yield only eventual errors. Complete lines buffered before an error are
    /// delivered as a partial batch first.
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
        let options = self.batch_options();
        let frames = self.execute_jsonl_frames_with_io(input, output).await?;
        Ok(batch_frames(frames, options))
    }

    /// The single process-lifecycle implementation. Batching and local line caps
    /// are layered above this primitive rather than duplicating execution logic.
    pub(crate) async fn execute_jsonl_frames_with_io(
        &mut self,
        input: &mut Input,
        output: &mut Output,
    ) -> Result<FrameStream, ExecutorError> {
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
                let mut lines = jsonl_frames(stdout);
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
    use alloc::{string::ToString, vec, vec::Vec};
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
                assert_eq!(
                    line.unwrap().unwrap().lines().collect::<Vec<_>>(),
                    vec![b"{}\n".to_vec()]
                );
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
                yield JsonlBatch::try_from(vec![b"{}".to_vec()]).unwrap();
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
            assert_eq!(
                stream
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .lines()
                    .collect::<Vec<_>>(),
                vec![b"{}\n".to_vec()]
            );
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
        let pid = std::str::from_utf8(pid.lines().next().unwrap())
            .unwrap()
            .trim();
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
                for _ in 0..64 {
                    yield JsonlBatch::try_from(vec![source_line.clone(); 64]).unwrap();
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
                let batch = actual.unwrap();
                for actual in batch.lines() {
                    assert_eq!(actual, line);
                }
                count += batch.len();
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
        .map(flatten_batches)
        .unwrap();
        assert_eq!(stream.next().await.unwrap().unwrap().as_bytes(), b"{}\r\n");
        assert_eq!(
            stream.next().await.unwrap().unwrap().as_bytes(),
            b"{\"last\":true}\n"
        );
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
            let input = Input::Jsonl(Box::pin(crate::stream::pending()));
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
            GraphInput::Jsonl(Box::pin(crate::stream::iter([
                Ok(JsonlBatch::default()),
                Ok(JsonlBatch::try_from(vec![b"{}".to_vec(), b"[]\r\n".to_vec()]).unwrap()),
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
        assert_eq!(
            stream
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            vec![b"{}\n".to_vec()]
        );
        assert!(stream.next().await.is_none());
    }
}
