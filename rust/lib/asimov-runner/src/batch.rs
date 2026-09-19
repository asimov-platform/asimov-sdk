// This is free and unencumbered software released into the public domain.

//! Bounded, latency-aware batches for Rust-facing JSONL streams.
//!
//! Batch boundaries are transport groupings, not RDF graphs or transactions, and
//! are not written to subprocess stdin/stdout. Native pipeline edges remain OS
//! pipes. Batch size and network-processing concurrency are separate controls:
//! callers can apply bounded concurrent stream adapters, preserving order when
//! processing sorted or paginated data.
//!
//! # An intermediate batch filter
//!
//! This example connects **Fetcher → Rust filter → Writer**. The filter receives
//! one batch at a time and forwards only records whose `@type` includes
//! `https://schema.org/Person`. It assumes one JSON-LD object per line, with
//! expanded type IRIs; it does not perform JSON-LD context expansion. Retained
//! lines keep their original bytes and ordering. The example uses `serde_json`
//! to inspect records; the stream traits and generator macro come from this crate.
//!
//! Unlike a direct native [`crate::Pipeline::pipe`] connection, an in-process
//! filter is passed to the next program as [`crate::GraphInput::Jsonl`]. Empty
//! filtered batches are skipped, but the source is still consumed to completion
//! so later errors are observed. Pulling batches supplies backpressure; processing
//! and upstream errors fail the downstream execution rather than silently
//! becoming an empty result.
//!
//! ```no_run
//! use asimov_runner::{
//!     AnyOutput, BatchOptions, ExecutorError, Fetcher, GraphInput, GraphOutput,
//!     JsonlBatch, JsonlStream, StreamExt, Writer, stream,
//! };
//! use serde_json::Value;
//! use std::{io, time::Duration};
//!
//! // Process an entire batch and retain only selected lines, without reserializing.
//! fn keep_people(batch: JsonlBatch) -> Result<JsonlBatch, ExecutorError> {
//!     let mut kept = Vec::new();
//!     for line in batch.into_lines() {
//!         let record: Value = serde_json::from_slice(&line)
//!             .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
//!         let wanted = "https://schema.org/Person";
//!         let keep = match record.get("@type") {
//!             Some(Value::String(kind)) => kind == wanted,
//!             Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind.as_str() == Some(wanted)),
//!             _ => false,
//!         };
//!         if keep {
//!             kept.push(line);
//!         }
//!     }
//!     Ok(JsonlBatch::new(kept))
//! }
//!
//! fn people_only(mut source: JsonlStream) -> JsonlStream {
//!     Box::pin(stream! {
//!         while let Some(batch) = source.next().await {
//!             match batch.and_then(keep_people) {
//!                 Ok(batch) if !batch.is_empty() => yield Ok(batch),
//!                 Ok(_) => {}, // All records in this batch were filtered out.
//!                 Err(error) => {
//!                     drop(source); // Release the producer before yielding the error.
//!                     yield Err(error);
//!                     return;
//!                 },
//!             }
//!         }
//!     })
//! }
//!
//! # async fn example() -> Result<(), ExecutorError> {
//! let batching = BatchOptions::new(128, 64 * 1024, Duration::from_millis(5))
//!     .expect("nonzero thresholds");
//! let source = Fetcher::new(
//!     "asimov-example-fetcher", "https://example.com/collection",
//!     GraphOutput::Captured, Default::default(),
//! ).with_batching(batching).execute().await?;
//!
//! let exported = Writer::new(
//!     "asimov-example-writer",
//!     GraphInput::Jsonl(people_only(source)),
//!     AnyOutput::Captured,
//!     Default::default(),
//! ).execute().await?.into_inner();
//! # Ok(())
//! # }
//! ```

use crate::{ExecutorError, Stream, StreamExt};
use alloc::{boxed::Box, vec::Vec};
use core::{
    num::{NonZeroUsize, TryFromIntError},
    pin::Pin,
    time::Duration,
};

/// Owned JSONL lines, retaining their original bytes and line endings.
///
/// Neither JSON nor UTF-8 is validated. An empty line is distinct from an empty
/// batch: graph inputs ignore empty batches but write an LF for an empty line.
/// SDK producers never emit empty batches.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JsonlBatch {
    lines: Vec<Vec<u8>>,
    byte_len: usize,
}

impl JsonlBatch {
    /// Takes ownership of lines without copying their bytes or validating them.
    pub fn new(lines: Vec<Vec<u8>>) -> Self {
        let byte_len = lines.iter().map(Vec::len).sum();
        Self { lines, byte_len }
    }

    /// Number of lines, including blank or unterminated lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the batch has no lines.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Total stored line bytes, including existing line endings. This excludes
    /// any LF a graph input may append to unterminated lines when writing them.
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Borrows the lines in their original order.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &[u8]> + DoubleEndedIterator {
        self.lines.iter().map(Vec::as_slice)
    }

    /// Returns the owned lines without copying their bytes.
    pub fn into_lines(self) -> Vec<Vec<u8>> {
        self.lines
    }

    fn push(&mut self, line: Vec<u8>) {
        self.byte_len += line.len();
        self.lines.push(line);
    }
}

impl From<Vec<Vec<u8>>> for JsonlBatch {
    fn from(lines: Vec<Vec<u8>>) -> Self {
        Self::new(lines)
    }
}

impl FromIterator<Vec<u8>> for JsonlBatch {
    fn from_iter<T: IntoIterator<Item = Vec<u8>>>(iter: T) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

/// Transport batching policy, independent of subprocess options and listing limits.
///
/// Defaults are 256 lines, a 256 KiB byte target, and 10 ms from adding the first
/// complete line to a new batch. The first threshold reached flushes the batch.
/// Counts are nonzero by construction. One oversized line is emitted alone;
/// lines are never split.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchOptions {
    /// Maximum lines per batch, not a total result limit.
    pub max_lines: NonZeroUsize,
    /// Typical maximum serialized batch size. A larger single line is allowed.
    pub target_bytes: NonZeroUsize,
    /// Maximum time spent collecting more lines after starting a batch,
    /// while the stream is being polled. Downstream backpressure still applies.
    /// Zero emits each line immediately. No empty timer batches are emitted.
    pub max_delay: Duration,
}

impl BatchOptions {
    /// Creates a policy, rejecting zero line/byte thresholds.
    ///
    /// ```
    /// use asimov_runner::BatchOptions;
    /// use std::time::Duration;
    /// let options = BatchOptions::new(128, 64 * 1024, Duration::from_millis(5))?;
    /// assert_eq!(options.max_lines.get(), 128);
    /// # Ok::<(), std::num::TryFromIntError>(())
    /// ```
    pub fn new(
        max_lines: usize,
        target_bytes: usize,
        max_delay: Duration,
    ) -> Result<Self, TryFromIntError> {
        Ok(Self {
            max_lines: NonZeroUsize::try_from(max_lines)?,
            target_bytes: NonZeroUsize::try_from(target_bytes)?,
            max_delay,
        })
    }
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self::new(256, 256 * 1024, Duration::from_millis(10)).expect("nonzero batch thresholds")
    }
}

/// A fallible stream of batches, with an implementation-specific error type.
pub type BatchStream<E = ExecutorError> = Pin<Box<dyn Stream<Item = Result<JsonlBatch, E>> + Send>>;

/// Individual lines for framing and line-at-a-time adapters.
pub type LineStream<E = ExecutorError> = Pin<Box<dyn Stream<Item = Result<Vec<u8>, E>> + Send>>;

/// Groups complete lines by count, byte target, or elapsed collection time.
///
/// Preserves order and bytes. EOF flushes a partial batch. On a source error,
/// the source is dropped immediately, buffered complete lines are yielded first,
/// and the error is yielded once as the final item. This does not prefetch while
/// the consumer holds a batch or delay cleanup after observing a source error.
///
/// Timed batching requires a Tokio runtime with time enabled, including when
/// the input is immediately ready.
/// Buffering is bounded by the configured batch plus at most one lookahead line
/// and the source's own buffers; individual line size is not bounded here.
pub fn batch_lines<E: Send + 'static>(
    source: impl Stream<Item = Result<Vec<u8>, E>> + Send + 'static,
    options: BatchOptions,
) -> BatchStream<E> {
    Box::pin(async_stream::stream! {
        let mut source = Box::pin(source);
        let mut lookahead = None;
        loop {
            let first = match lookahead.take() {
                Some(line) => Some(Ok(line)),
                None => source.next().await,
            };
            let first = match first {
                Some(Ok(line)) => line,
                Some(Err(error)) => {
                    drop(source);
                    yield Err(error);
                    return;
                },
                None => return,
            };
            let mut batch = JsonlBatch::default();
            batch.push(first);
            let deadline = tokio::time::Instant::now().checked_add(options.max_delay);
            let timer = async {
                match deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => core::future::pending::<()>().await,
                }
            };
            tokio::pin!(timer);
            let mut terminal = None;
            while batch.len() < options.max_lines.get()
                && batch.byte_len() < options.target_bytes.get()
                && !deadline.is_some_and(|deadline| tokio::time::Instant::now() >= deadline)
            {
                let next = tokio::select! {
                    biased;
                    _ = &mut timer => break,
                    next = source.next() => next,
                };
                match next {
                    Some(Ok(line)) => {
                        if line.len() > options.target_bytes.get() - batch.byte_len() {
                            lookahead = Some(line);
                            break;
                        }
                        batch.push(line);
                    },
                    Some(Err(error)) => {
                        terminal = Some(Err(error));
                        break;
                    },
                    None => {
                        terminal = Some(Ok(()));
                        break;
                    },
                }
            }
            if let Some(terminal) = terminal {
                // Release process/pipe owners before suspending at the partial
                // batch, rather than waiting for another downstream poll.
                drop(source);
                yield Ok(batch);
                if let Err(error) = terminal {
                    yield Err(error);
                }
                return;
            }
            yield Ok(batch);
        }
    })
}

/// Adapts batches to individual owned lines without copying their bytes.
/// Empty batches are skipped. Order, line endings, and terminal errors are preserved.
pub fn flatten_batches<E: Send + 'static>(
    source: impl Stream<Item = Result<JsonlBatch, E>> + Send + 'static,
) -> LineStream<E> {
    Box::pin(async_stream::stream! {
        let mut source = Box::pin(source);
        while let Some(batch) = source.next().await {
            match batch {
                Ok(batch) => {
                    if batch.is_empty() {
                        tokio::task::yield_now().await;
                    }
                    for line in batch.into_lines() {
                        yield Ok(line);
                    }
                },
                Err(error) => {
                    drop(source);
                    yield Err(error);
                    return;
                },
            }
        }
    })
}

macro_rules! with_batching {
    ($program:ty) => {
        impl $program {
            /// Sets batching thresholds for captured JSONL output. This does not change
            /// subprocess arguments, native pipeline edges, or listing limits.
            /// The default policy is [`crate::BatchOptions::default`].
            #[must_use]
            pub fn with_batching(mut self, options: crate::BatchOptions) -> Self {
                self.executor = self.executor.with_batching(options);
                self
            }
        }
    };
}
pub(crate) use with_batching;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use core::{
        convert::Infallible,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        task::{Context, Poll},
    };
    use std::{io, sync::Arc};
    use tokio::{
        io::{AsyncRead, AsyncWriteExt, ReadBuf},
        time::{Instant, timeout},
    };

    fn options(lines: usize, bytes: usize, delay: Duration) -> BatchOptions {
        BatchOptions::new(lines, bytes, delay).unwrap()
    }

    fn lines(values: &[&[u8]]) -> LineStream<Infallible> {
        Box::pin(crate::stream::iter(
            values
                .iter()
                .map(|line| Ok(line.to_vec()))
                .collect::<Vec<_>>(),
        ))
    }

    #[test]
    fn validates_thresholds_and_reports_batch_dimensions() {
        assert!(BatchOptions::new(0, 1, Duration::ZERO).is_err());
        assert!(BatchOptions::new(1, 0, Duration::ZERO).is_err());
        assert_eq!(BatchOptions::default().max_lines.get(), 256);
        assert_eq!(BatchOptions::default().target_bytes.get(), 256 * 1024);
        assert_eq!(BatchOptions::default().max_delay, Duration::from_millis(10));
        let batch = JsonlBatch::new(vec![b"{}\r\n".to_vec(), Vec::new(), b"tail".to_vec()]);
        assert_eq!(batch.len(), 3);
        assert_eq!(batch.byte_len(), 8);
        assert!(!batch.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn line_threshold_and_backpressure_do_not_prefetch_more_batches() {
        let read = Arc::new(AtomicUsize::new(0));
        let counter = read.clone();
        let source = lines(&[b"a\n", b"b\n", b"c\n", b"d\n", b"e"]).map(move |line| {
            counter.fetch_add(1, Ordering::SeqCst);
            line
        });
        let mut batches = batch_lines(source, options(2, 1024, Duration::from_secs(1)));
        assert_eq!(batches.next().await.unwrap().unwrap().len(), 2);
        assert_eq!(read.load(Ordering::SeqCst), 2);
        tokio::time::advance(Duration::from_secs(5)).await;
        assert_eq!(
            read.load(Ordering::SeqCst),
            2,
            "holding a batch must backpressure the source"
        );
        assert_eq!(batches.next().await.unwrap().unwrap().len(), 2);
        assert_eq!(
            batches.next().await.unwrap().unwrap().into_lines(),
            vec![b"e".to_vec()]
        );
        assert!(batches.next().await.is_none());
        assert!(batches.next().await.is_none());
    }

    #[tokio::test]
    async fn byte_target_uses_complete_lines_and_allows_oversized_singletons() {
        let mut batches = batch_lines(
            lines(&[b"a\n", b"b\n", b"ccc\n", b"oversized", b"z"]),
            options(100, 5, Duration::from_secs(1)),
        );
        for expected in [
            vec![b"a\n".to_vec(), b"b\n".to_vec()],
            vec![b"ccc\n".to_vec()],
            vec![b"oversized".to_vec()],
            vec![b"z".to_vec()],
        ] {
            assert_eq!(
                batches.next().await.unwrap().unwrap().into_lines(),
                expected
            );
        }
        assert!(batches.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn flushes_sparse_stream_on_deadline_without_waiting_for_eof() {
        let source = lines(&[b"first\n"]).chain(crate::stream::pending());
        let mut batches = batch_lines(source, options(100, 1024, Duration::from_millis(10)));
        let start = Instant::now();
        assert_eq!(
            batches.next().await.unwrap().unwrap().into_lines(),
            vec![b"first\n".to_vec()]
        );
        assert_eq!(start.elapsed(), Duration::from_millis(10));
    }

    #[tokio::test(start_paused = true)]
    async fn never_emits_empty_batches_and_zero_delay_emits_immediately() {
        let mut pending = batch_lines(
            crate::stream::pending::<Result<Vec<u8>, Infallible>>(),
            BatchOptions::default(),
        );
        assert!(
            timeout(Duration::from_secs(1), pending.next())
                .await
                .is_err()
        );
        let mut empty = batch_lines(lines(&[]), BatchOptions::default());
        assert!(empty.next().await.is_none());
        let mut ready = batch_lines(lines(&[b"a", b"b"]), options(100, 1024, Duration::ZERO));
        let start = Instant::now();
        assert_eq!(ready.next().await.unwrap().unwrap().len(), 1);
        assert_eq!(ready.next().await.unwrap().unwrap().len(), 1);
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_does_not_discard_a_partially_read_next_line() {
        let (reader, mut writer) = tokio::io::duplex(128);
        let writing = tokio::spawn(async move {
            writer.write_all(b"{}\n{\"pa").await.unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
            writer.write_all(b"rt\":true}\n").await.unwrap();
        });
        let mut batches =
            crate::jsonl_batches(reader, options(100, 1024, Duration::from_millis(5)));
        let start = Instant::now();
        assert_eq!(
            batches.next().await.unwrap().unwrap().into_lines(),
            vec![b"{}\n".to_vec()]
        );
        assert_eq!(start.elapsed(), Duration::from_millis(5));
        assert_eq!(
            batches.next().await.unwrap().unwrap().into_lines(),
            vec![b"{\"part\":true}\n".to_vec()]
        );
        assert!(batches.next().await.is_none());
        writing.await.unwrap();
    }

    struct FailingSource {
        step: usize,
        dropped: Arc<AtomicBool>,
    }
    impl Drop for FailingSource {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    impl Stream for FailingSource {
        type Item = Result<Vec<u8>, &'static str>;
        fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.step += 1;
            Poll::Ready(Some(match self.step {
                1 => Ok(b"first\n".to_vec()),
                2 => Err("source failed"),
                _ => panic!("source must not be polled after failure"),
            }))
        }
    }

    #[tokio::test]
    async fn flushes_partial_batch_before_error_but_drops_source_before_yielding() {
        let dropped = Arc::new(AtomicBool::new(false));
        let mut batches = batch_lines(
            FailingSource {
                step: 0,
                dropped: dropped.clone(),
            },
            BatchOptions::default(),
        );
        assert_eq!(
            batches.next().await.unwrap().unwrap().into_lines(),
            vec![b"first\n".to_vec()]
        );
        assert!(
            dropped.load(Ordering::SeqCst),
            "error cleanup must not wait for the consumer's next poll"
        );
        assert_eq!(batches.next().await.unwrap().unwrap_err(), "source failed");
        assert!(batches.next().await.is_none());
    }

    struct FailingReader(bool);
    impl AsyncRead for FailingReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.0 {
                return Poll::Ready(Err(io::Error::other("read failed")));
            }
            self.0 = true;
            buffer.put_slice(b"{}\npartial");
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn read_error_never_emits_an_incomplete_line() {
        let mut batches = crate::jsonl_batches(FailingReader(false), BatchOptions::default());
        assert_eq!(
            batches.next().await.unwrap().unwrap().into_lines(),
            vec![b"{}\n".to_vec()]
        );
        assert!(batches.next().await.unwrap().is_err());
        assert!(batches.next().await.is_none());
    }

    #[tokio::test]
    async fn flattening_round_trips_raw_lines_and_ignores_empty_batches() {
        let expected = [b"{}\n".as_slice(), b"\r\n", b"\xff\n", b"tail"];
        let source = batch_lines(lines(&expected), options(2, 1024, Duration::from_secs(1)));
        let empty = crate::stream::iter([Ok(JsonlBatch::default())]);
        let mut flattened = flatten_batches(empty.chain(source));
        for line in expected {
            assert_eq!(flattened.next().await.unwrap().unwrap(), line);
        }
        assert!(flattened.next().await.is_none());
    }
}
