// This is free and unencumbered software released into the public domain.

//! Byte and JSONL batch sources for a child process's standard input.
//!
//! The content-specific aliases all refer to [`Input`]; they express a program
//! pattern's expected payload without imposing an encoding or validating bytes.
//! [`NoInput`] instead represents a pattern that has no input parameter.

use alloc::boxed::Box;
use derive_more::Debug;
use tokio::io::AsyncRead;

/// An input stream with no prescribed content type.
pub type AnyInput = Input;
/// JSONL graph input. With `std`, graph consumers adapt [`Input::AsyncRead`] into
/// line batches; `Input::Jsonl` connects a graph producer's output directly to a consumer.
pub type GraphInput = Input;
/// The absence of an input value for a program pattern.
pub type NoInput = ();
/// An input stream intended to contain a SPARQL query.
pub type QueryInput = Input;
/// An input stream intended to contain text, without enforcing an encoding.
pub type TextInput = Input;

/// The source of bytes to supply to a child's stdin.
///
/// Program wrappers configure the child's stdin from this value and feed the
/// resulting pipe during execution. Graph-output runners transfer ownership of
/// their input into the returned stream. Buffered runners consume it in place.
/// Repeated executions never replay bytes that have already been read. Early
/// exit or cancellation can leave input partially consumed, including a partly
/// written record; reusing it does not guarantee record-boundary resumption.
///
/// With `std` enabled, conversion to `Stdio` only selects null or piped stdin;
/// it does not transfer bytes. The consuming conversion also drops any stored
/// reader or batch stream. Use `Input::as_stdio` to preserve the source for execution.
#[derive(Debug)]
pub enum Input {
    /// Supplies no bytes by configuring stdin to read from the null device.
    Ignored,
    /// Supplies bytes from an owned asynchronous reader through a stdin pipe.
    ///
    /// The reader must support use across asynchronous tasks (`Send + Sync`)
    /// and unpinned I/O (`Unpin`). Its contents are omitted from debug output.
    AsyncRead(#[debug(skip)] Box<dyn AsyncRead + Send + Sync + Unpin>),
    /// Supplies JSONL batches, applying backpressure and propagating source errors.
    /// Lines are coalesced into a reusable write buffer for each batch. Batch
    /// boundaries are not encoded on the wire. Empty batches are ignored.
    /// Existing line endings are preserved; an LF is appended to any line that
    /// does not end in LF so adjacent records cannot run together.
    /// An empty line therefore writes a blank line. Lines are not checked for
    /// embedded newlines, valid JSON, UTF-8, or an RDF mapping profile.
    #[cfg(feature = "std")]
    Jsonl(#[debug(skip)] crate::JsonlStream),
}

impl Input {
    /// Selects the child's stdin configuration without consuming this input.
    ///
    /// Returns null stdin for [`Ignored`](Self::Ignored) and a pipe for
    /// [`AsyncRead`](Self::AsyncRead) and [`Jsonl`](Self::Jsonl). The caller must
    /// still feed the child's pipe after spawning it.
    #[cfg(feature = "std")]
    pub fn as_stdio(&self) -> std::process::Stdio {
        use std::process::Stdio;
        match self {
            Input::Ignored => Stdio::null(),
            Input::AsyncRead(_) => Stdio::piped(),
            Input::Jsonl(_) => Stdio::piped(),
        }
    }

    /// Adapts byte input to batched JSONL input using default thresholds.
    ///
    /// Wraps [`AsyncRead`](Self::AsyncRead) using [`crate::jsonl_batches`]; other
    /// variants are returned unchanged. Adaptation is lazy and performs no I/O.
    /// When fed to a child, a final unterminated line gains an LF as described
    /// by [`Jsonl`](Self::Jsonl).
    #[cfg(feature = "std")]
    pub fn into_jsonl(self) -> Self {
        self.into_jsonl_with_batching(crate::BatchOptions::default())
    }

    /// Adapts an asynchronous reader into JSONL batches with the supplied policy.
    /// Existing batch streams and ignored input are returned unchanged. To rebatch
    /// a stream, combine [`crate::flatten_batches`] and [`crate::batch_lines`].
    #[cfg(feature = "std")]
    pub fn into_jsonl_with_batching(self, options: crate::BatchOptions) -> Self {
        match self {
            Self::AsyncRead(reader) => Self::Jsonl(crate::jsonl_batches(reader, options)),
            input => input,
        }
    }

    #[cfg(feature = "std")]
    pub(crate) async fn write_to(
        &mut self,
        stdin: Option<tokio::process::ChildStdin>,
    ) -> Result<(), crate::completion::InputFailure> {
        use crate::completion::InputFailure;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        if matches!(self, Self::Ignored) {
            return Ok(());
        }
        let mut stdin = stdin.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "stdin must be piped")
        })?;
        match self {
            Self::Ignored => {},
            Self::AsyncRead(reader) => {
                let mut buffer = [0; 8192];
                loop {
                    let count = reader
                        .read(&mut buffer)
                        .await
                        .map_err(|error| InputFailure::Source(error.into()))?;
                    if count == 0 {
                        break;
                    }
                    stdin.write_all(&buffer[..count]).await?;
                }
            },
            Self::Jsonl(batches) => {
                write_batches(batches, &mut stdin).await?;
            },
        }
        stdin.shutdown().await?;
        Ok(())
    }
}

#[cfg(feature = "std")]
async fn write_batches(
    batches: &mut crate::JsonlStream,
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> Result<(), crate::completion::InputFailure> {
    use crate::completion::InputFailure;
    use futures_lite::StreamExt;
    use tokio::io::AsyncWriteExt;

    let mut buffer = alloc::vec::Vec::new();
    while let Some(batch) = batches.next().await {
        let batch = batch.map_err(InputFailure::Source)?;
        if batch.is_empty() {
            tokio::task::yield_now().await;
            continue;
        }
        buffer.clear();
        for line in batch.lines() {
            buffer.extend_from_slice(line);
            if !line.ends_with(b"\n") {
                buffer.push(b'\n');
            }
        }
        // write_all handles partial writes and backpressure. There is no
        // per-line flush or additional batch framing on the wire.
        writer.write_all(&buffer).await?;
    }
    Ok(())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::{ExecutorError, JsonlBatch, JsonlStream, completion::InputFailure};
    use alloc::{vec, vec::Vec};
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use std::io;
    use tokio::io::AsyncWrite;

    struct Destination {
        bytes: Vec<u8>,
        writes: usize,
        max_write: usize,
    }
    impl AsyncWrite for Destination {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let count = bytes.len().min(self.max_write);
            self.bytes.extend_from_slice(&bytes[..count]);
            self.writes += 1;
            Poll::Ready(Ok(count))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            panic!("batches must not flush per line");
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn coalesces_batches_preserving_line_endings_and_handling_partial_writes() {
        for max_write in [usize::MAX, 3] {
            let mut batches: JsonlStream = Box::pin(futures_lite::stream::iter([
                Ok(JsonlBatch::default()),
                Ok(JsonlBatch::new(vec![
                    b"{}".to_vec(),
                    b"[]\r\n".to_vec(),
                    Vec::new(),
                ])),
                Ok(JsonlBatch::new(vec![b"last".to_vec()])),
            ]));
            let mut destination = Destination {
                bytes: Vec::new(),
                writes: 0,
                max_write,
            };
            assert!(write_batches(&mut batches, &mut destination).await.is_ok());
            assert_eq!(destination.bytes, b"{}\n[]\r\n\nlast\n");
            if max_write == usize::MAX {
                assert_eq!(destination.writes, 2);
            }
        }
    }

    #[tokio::test]
    async fn batch_source_error_stops_writing_after_complete_batches() {
        let mut batches: JsonlStream = Box::pin(futures_lite::stream::iter([
            Ok(JsonlBatch::new(vec![b"first".to_vec()])),
            Err(ExecutorError::UnexpectedOther(io::Error::other(
                "source failed",
            ))),
            Ok(JsonlBatch::new(vec![b"must not be written".to_vec()])),
        ]));
        let mut destination = Destination {
            bytes: Vec::new(),
            writes: 0,
            max_write: usize::MAX,
        };
        assert!(matches!(
            write_batches(&mut batches, &mut destination).await,
            Err(InputFailure::Source(_))
        ));
        assert_eq!(destination.bytes, b"first\n");
    }
}

#[cfg(feature = "std")]
impl Into<std::process::Stdio> for Input {
    fn into(self) -> std::process::Stdio {
        use std::process::Stdio;
        match self {
            Input::Ignored => Stdio::null(),
            Input::AsyncRead(_) => Stdio::piped(),
            Input::Jsonl(_) => Stdio::piped(),
        }
    }
}
