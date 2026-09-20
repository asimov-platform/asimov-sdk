// This is free and unencumbered software released into the public domain.

//! Lazy JSONL framing for byte readers and fallible transport streams.
//!
//! Framing preserves LF/CRLF, blank lines, and a final unterminated line. Neither
//! UTF-8 nor JSON is parsed. Partial lines are discarded on transport failure.
//! Memory follows the current line, read chunk, and configured batch thresholds;
//! an individual line has no size bound. Dropping a stream releases its source.
//! Chunk-to-line framing is runtime-independent. Reader adapters and timed batch
//! accumulation require the `tokio` feature.

use crate::batch::{FrameStream, FramedLine};
#[cfg(feature = "tokio")]
use crate::{BatchOptions, BatchStream, batch::batch_frames};
use crate::{Bytes, BytesMut, LineStream, Stream, StreamExt};
use alloc::boxed::Box;
#[cfg(feature = "tokio")]
use bytes::BufMut;
#[cfg(feature = "tokio")]
use tokio::io::{AsyncRead, AsyncReadExt};

#[cfg(feature = "tokio")]
pub fn jsonl_lines(reader: impl AsyncRead + Send + Unpin + 'static) -> LineStream<std::io::Error> {
    Box::pin(jsonl_frames(reader).map(|line| line.map(FramedLine::into_line)))
}

#[cfg(feature = "tokio")]
pub fn jsonl_batches(
    reader: impl AsyncRead + Send + Unpin + 'static,
    options: BatchOptions,
) -> BatchStream<std::io::Error> {
    batch_frames(jsonl_frames(reader), options)
}

/// Frames and batches arbitrary transport chunks, preserving the source error.
/// A network chunk is not assumed to contain complete JSONL records.
#[cfg(feature = "tokio")]
pub fn jsonl_batches_from_chunks<E: Send + 'static>(
    chunks: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
    options: BatchOptions,
) -> BatchStream<E> {
    batch_frames(frames_from_chunks(chunks), options)
}

/// Frames arbitrary transport chunks into lines without requiring an async runtime.
/// Preserves bytes and line endings, and yields a source error once before ending.
pub fn jsonl_lines_from_chunks<E: Send + 'static>(
    chunks: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> LineStream<E> {
    Box::pin(frames_from_chunks(chunks).map(|line| line.map(FramedLine::into_line)))
}

#[doc(hidden)]
#[cfg(feature = "tokio")]
pub fn jsonl_frames(
    reader: impl AsyncRead + Send + Unpin + 'static,
) -> FrameStream<std::io::Error> {
    Box::pin(async_stream::try_stream! {
        const READ_CHUNK: usize = 16 * 1024;
        let mut reader = reader;
        let mut buffer = BytesMut::with_capacity(READ_CHUNK);
        let mut scanned = 0;
        loop {
            if let Some(last) = memchr::memrchr(b'\n', &buffer[scanned..]) {
                let complete = buffer.split_to(scanned + last + 1).freeze();
                scanned = 0;
                let mut start = 0;
                for newline in memchr::memchr_iter(b'\n', &complete) {
                    yield FramedLine::new(complete.clone(), start..newline + 1);
                    start = newline + 1;
                }
                continue;
            }
            scanned = buffer.len();
            if buffer.capacity() == buffer.len() { buffer.reserve(READ_CHUNK); }
            if reader.read_buf(&mut (&mut buffer).limit(READ_CHUNK)).await? == 0 {
                if !buffer.is_empty() {
                    let len = buffer.len();
                    yield FramedLine::new(buffer.freeze(), 0..len);
                }
                break;
            }
        }
    })
}

fn frames_from_chunks<E: Send + 'static>(
    chunks: impl Stream<Item = Result<Bytes, E>> + Send + 'static,
) -> FrameStream<E> {
    Box::pin(async_stream::stream! {
        let mut chunks = Box::pin(chunks);
        let mut partial = BytesMut::new();
        while let Some(chunk) = chunks.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    drop(chunks);
                    yield Err(error);
                    return;
                },
            };
            let mut start = 0;
            for newline in memchr::memchr_iter(b'\n', &chunk) {
                let end = newline + 1;
                let line = if partial.is_empty() {
                    FramedLine::new(chunk.clone(), start..end)
                } else {
                    partial.extend_from_slice(&chunk[start..end]);
                    let len = partial.len();
                    FramedLine::new(partial.split().freeze(), 0..len)
                };
                start = end;
                yield Ok(line);
            }
            partial.extend_from_slice(&chunk[start..]);
        }
        drop(chunks);
        if !partial.is_empty() {
            let len = partial.len();
            yield Ok(FramedLine::new(partial.freeze(), 0..len));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use core::convert::Infallible;

    #[test]
    fn arbitrary_chunk_boundaries_preserve_all_bytes() {
        futures_lite::future::block_on(async {
            let input = b"\n{\"name\":\"caf\xc3\xa9\"}\r\n\xff\nlast";
            for width in 1..=input.len() {
                let chunks: Vec<_> = input
                    .chunks(width)
                    .map(|chunk| Ok::<_, Infallible>(Bytes::copy_from_slice(chunk)))
                    .collect();
                let mut lines = jsonl_lines_from_chunks(crate::stream::iter(chunks));
                for expected in [
                    b"\n".as_slice(),
                    b"{\"name\":\"caf\xc3\xa9\"}\r\n",
                    b"\xff\n",
                    b"last",
                ] {
                    assert_eq!(lines.next().await.unwrap().unwrap().as_bytes(), expected);
                }
                assert!(lines.next().await.is_none());
            }
        });
    }

    #[test]
    fn chunk_error_discards_partial_line_and_terminates() {
        futures_lite::future::block_on(async {
            let mut lines = jsonl_lines_from_chunks(crate::stream::iter([
                Ok(Bytes::from_static(b"{}\npartial")),
                Err("body failed"),
                Ok(Bytes::from_static(b"unreachable\n")),
            ]));
            assert_eq!(lines.next().await.unwrap().unwrap().as_bytes(), b"{}\n");
            assert_eq!(lines.next().await.unwrap().unwrap_err(), "body failed");
            assert!(lines.next().await.is_none());
        });
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn flushes_complete_records_before_source_error() {
        let chunks = crate::stream::iter([
            Ok(Bytes::from_static(b"{}\npartial")),
            Err("body failed"),
            Ok(Bytes::from_static(b"unreachable\n")),
        ]);
        let mut batches = jsonl_batches_from_chunks(chunks, BatchOptions::default());
        assert_eq!(
            batches.next().await.unwrap().unwrap().as_contiguous_bytes(),
            Some(b"{}\n".as_slice())
        );
        assert_eq!(batches.next().await.unwrap().unwrap_err(), "body failed");
        assert!(batches.next().await.is_none());
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn complete_chunk_lines_keep_shared_contiguous_storage() {
        let bytes = Bytes::from_static(b"a\nb\n");
        let pointer = bytes.as_ptr();
        let mut batches = jsonl_batches_from_chunks(
            crate::stream::iter([Ok::<_, Infallible>(bytes)]),
            BatchOptions::default(),
        );
        let batch = batches.next().await.unwrap().unwrap();
        assert_eq!(batch.as_contiguous_bytes().unwrap().as_ptr(), pointer);
        assert_eq!(batch.len(), 2);
        assert!(batches.next().await.is_none());
    }
}
