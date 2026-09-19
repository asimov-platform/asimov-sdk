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
//! Filtering moves validated `JsonlLine` values without copying their payloads.
//! For sparse selections held in long-lived queues or caches, call
//! [`JsonlBatch::into_compact`] on the result to release larger shared buffers.
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

use crate::{Bytes, ExecutorError, JsonlLine, JsonlLineError, Stream, StreamExt};
use alloc::{boxed::Box, vec::Vec};
use core::{
    iter::FusedIterator,
    num::{NonZeroUsize, TryFromIntError},
    ops::Range,
    pin::Pin,
    time::Duration,
};

/// Validated JSONL lines, retaining their original bytes and line endings.
///
/// Public [`JsonlLine`] constructors validate framing. Neither JSON nor UTF-8 is
/// validated. An empty line is distinct from an empty
/// batch: graph inputs ignore empty batches but write an LF for an empty line.
/// SDK producers never emit empty batches.
///
/// Storage is private: reader-produced batches can retain one `Bytes` view plus
/// line-end offsets, while batches assembled from individual lines preserve
/// those line values. Borrowed iteration does not materialize individual shared
/// handles. Extracting lines produces ordinary `Bytes` slices; rebuilding a
/// batch from them does not infer allocation identity or recover a wider view.
///
/// ```
/// use asimov_runner::{Bytes, JsonlBatch, JsonlLine, JsonlLineError};
/// let batch = JsonlBatch::new(vec![
///     JsonlLine::owned(b"{}\n".to_vec())?,
///     JsonlLine::shared(Bytes::from_static(b"[]\r\n"))?,
/// ]);
/// assert_eq!(batch.len(), 2);
/// let raw = JsonlBatch::try_from(vec![b"{}\n".to_vec()])?;
/// assert_eq!(raw.byte_len(), 3);
/// # Ok::<(), JsonlLineError>(())
/// ```
#[derive(Clone, Debug)]
pub struct JsonlBatch {
    storage: BatchStorage,
    byte_len: usize,
}

#[derive(Clone, Debug)]
enum BatchStorage {
    // Bytes is the exact batch view. End offsets are relative to that view and
    // preserve even empty/unterminated line boundaries. No per-line Bytes handles
    // are created until a caller extracts owned lines.
    Contiguous { bytes: Bytes, line_ends: Vec<usize> },
    Lines(Vec<JsonlLine>),
}

impl Default for JsonlBatch {
    fn default() -> Self {
        Self {
            storage: BatchStorage::Lines(Vec::new()),
            byte_len: 0,
        }
    }
}

impl PartialEq for JsonlBatch {
    fn eq(&self, other: &Self) -> bool {
        self.byte_len == other.byte_len
            && self.len() == other.len()
            && self.lines().eq(other.lines())
    }
}
impl Eq for JsonlBatch {}

impl JsonlBatch {
    /// Takes ownership of already-validated lines without copying their bytes.
    /// Panics if their aggregate stored byte length overflows `usize`.
    pub fn new(lines: Vec<JsonlLine>) -> Self {
        let byte_len = lines
            .iter()
            .try_fold(0usize, |total, line| total.checked_add(line.len()))
            .expect("JSONL batch byte length exceeds usize");
        Self {
            storage: BatchStorage::Lines(lines),
            byte_len,
        }
    }

    /// Frames a complete JSONL byte buffer into a contiguous batch without copying
    /// its payload. Retains LF/CRLF endings and a final nonempty unterminated line;
    /// an empty buffer means an empty batch. JSON and UTF-8 are not validated.
    ///
    /// Use this for a complete buffer, not arbitrary I/O chunks that can split a
    /// line. Streaming readers use [`crate::jsonl_batches`] to retain partial lines.
    ///
    /// ```
    /// use asimov_runner::{Bytes, JsonlBatch};
    /// let batch = JsonlBatch::from_bytes(Bytes::from_static(b"{}\n[]\r\n"));
    /// assert_eq!(batch.len(), 2);
    /// assert_eq!(batch.as_contiguous_bytes(), Some(b"{}\n[]\r\n".as_slice()));
    /// let lines = batch.into_lines(); // Cheap shared views of individual lines.
    /// assert_eq!(lines[1].content(), b"[]");
    /// ```
    pub fn from_bytes(bytes: Bytes) -> Self {
        let mut line_ends: Vec<_> = memchr::memchr_iter(b'\n', &bytes)
            .map(|index| index + 1)
            .collect();
        if !bytes.is_empty() && line_ends.last().copied() != Some(bytes.len()) {
            line_ends.push(bytes.len());
        }
        Self::contiguous(bytes, line_ends)
    }

    fn contiguous(bytes: Bytes, line_ends: Vec<usize>) -> Self {
        debug_assert_eq!(line_ends.last().copied().unwrap_or(0), bytes.len());
        debug_assert!({
            let mut start = 0;
            line_ends.iter().all(|&end| {
                let valid = JsonlLine::shared_slice(bytes.clone(), start..end).is_ok();
                start = end;
                valid
            })
        });
        Self {
            byte_len: bytes.len(),
            storage: BatchStorage::Contiguous { bytes, line_ends },
        }
    }

    /// Number of lines, including blank or unterminated lines.
    pub fn len(&self) -> usize {
        match &self.storage {
            BatchStorage::Contiguous { line_ends, .. } => line_ends.len(),
            BatchStorage::Lines(lines) => lines.len(),
        }
    }

    /// Whether the batch has no lines.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total stored line bytes, including existing line endings. This excludes
    /// any LF a graph input may append to unterminated lines when writing them.
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    /// Borrows byte views of the lines in their original order. Use
    /// [`into_lines`](Self::into_lines) when individual lines need owned lifetimes.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = &[u8]> + DoubleEndedIterator {
        BatchLines {
            batch: self,
            front: 0,
            back: self.len(),
        }
    }

    /// Returns owned line values without copying payload bytes. Contiguous batches
    /// create shared `Bytes` slices here; independently constructed lines retain
    /// their existing storage mode. Shared values may retain larger allocations.
    pub fn into_lines(self) -> Vec<JsonlLine> {
        match self.storage {
            BatchStorage::Lines(lines) => lines,
            BatchStorage::Contiguous { bytes, line_ends } => {
                let mut start = 0;
                line_ends
                    .into_iter()
                    .map(|end| {
                        let line = JsonlLine::framed(bytes.slice(start..end));
                        start = end;
                        line
                    })
                    .collect()
            },
        }
    }

    fn line_bytes(&self, index: usize) -> &[u8] {
        match &self.storage {
            BatchStorage::Lines(lines) => lines[index].as_bytes(),
            BatchStorage::Contiguous { bytes, line_ends } => {
                let start = if index == 0 { 0 } else { line_ends[index - 1] };
                &bytes[start..line_ends[index]]
            },
        }
    }

    /// Returns a ready-to-write contiguous JSONL encoding without copying, when
    /// all lines are terminated and the batch retains a contiguous backing view
    /// (or contains just one line). Batches made from separate lines do not infer
    /// shared allocation identity from pointer adjacency. Unterminated lines need
    /// LF insertion and return `None`. Empty batches return an empty slice.
    pub fn as_contiguous_bytes(&self) -> Option<&[u8]> {
        if self.is_empty() {
            return Some(&[]);
        }
        match &self.storage {
            BatchStorage::Contiguous { bytes, .. }
                if self.lines().all(|line| line.ends_with(b"\n")) =>
            {
                Some(bytes)
            },
            BatchStorage::Lines(lines) if lines.len() == 1 && lines[0].is_terminated() => {
                Some(lines[0].as_bytes())
            },
            _ => None,
        }
    }

    /// Copies stored bytes into one compact backing buffer, preserving line
    /// boundaries and endings. This releases references to larger read buffers
    /// when a filter keeps only a small subset. `byte_len` measures logical bytes,
    /// not the allocation size retained by shared lines.
    pub fn into_compact(self) -> Self {
        let mut bytes = Vec::with_capacity(self.byte_len);
        let mut line_ends = Vec::with_capacity(self.len());
        for line in self.lines() {
            bytes.extend_from_slice(line);
            line_ends.push(bytes.len());
        }
        Self::contiguous(Bytes::from(bytes), line_ends)
    }

    /// Builds a bounded set of wire slices, including missing LF terminators.
    /// Highly fragmented batches fall back to the caller's reusable copy buffer.
    pub(crate) fn wire_slices(&self, maximum: usize) -> Option<Vec<std::io::IoSlice<'_>>> {
        use std::io::IoSlice;
        let mut slices = Vec::new();
        match &self.storage {
            BatchStorage::Lines(lines) => {
                for line in lines {
                    if !line.is_empty() {
                        slices.push(IoSlice::new(line.as_bytes()));
                    }
                    if !line.is_terminated() {
                        slices.push(IoSlice::new(b"\n"));
                    }
                    if slices.len() > maximum {
                        return None;
                    }
                }
            },
            BatchStorage::Contiguous { bytes, line_ends } => {
                let mut start = 0;
                let mut run_start = 0;
                for &end in line_ends {
                    if !bytes[start..end].ends_with(b"\n") {
                        if end != run_start {
                            slices.push(IoSlice::new(&bytes[run_start..end]));
                        }
                        slices.push(IoSlice::new(b"\n"));
                        run_start = end;
                    }
                    if slices.len() > maximum {
                        return None;
                    }
                    start = end;
                }
                if run_start < bytes.len() {
                    slices.push(IoSlice::new(&bytes[run_start..]));
                }
                if slices.len() > maximum {
                    return None;
                }
            },
        }
        Some(slices)
    }
}

struct BatchLines<'a> {
    batch: &'a JsonlBatch,
    front: usize,
    back: usize,
}

impl<'a> Iterator for BatchLines<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let index = self.front;
        self.front += 1;
        Some(self.batch.line_bytes(index))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }
}
impl DoubleEndedIterator for BatchLines<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        Some(self.batch.line_bytes(self.back))
    }
}
impl ExactSizeIterator for BatchLines<'_> {}
impl FusedIterator for BatchLines<'_> {}

/// Provenance used only between the framer and batch builder. It is deliberately
/// not part of the public JsonlLine value passed to application code.
pub(crate) enum FramedLine {
    Line(JsonlLine),
    Buffer { bytes: Bytes, range: Range<usize> },
}

impl FramedLine {
    pub(crate) fn new(bytes: Bytes, range: Range<usize>) -> Self {
        debug_assert!(JsonlLine::shared_slice(bytes.clone(), range.clone()).is_ok());
        Self::Buffer { bytes, range }
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Line(line) => line.as_bytes(),
            Self::Buffer { bytes, range } => &bytes[range.clone()],
        }
    }
    fn len(&self) -> usize {
        self.as_bytes().len()
    }
    pub(crate) fn into_line(self) -> JsonlLine {
        match self {
            Self::Line(line) => line,
            Self::Buffer { bytes, range } => JsonlLine::framed(bytes.slice(range)),
        }
    }
}

pub(crate) type FrameStream<E = ExecutorError> =
    Pin<Box<dyn Stream<Item = Result<FramedLine, E>> + Send>>;

#[derive(Default)]
enum BuilderStorage {
    #[default]
    Empty,
    Buffer {
        bytes: Bytes,
        start: usize,
        line_ends: Vec<usize>,
    },
    Lines(Vec<JsonlLine>),
}

impl BuilderStorage {
    fn finish(self, byte_len: usize) -> JsonlBatch {
        match self {
            Self::Empty => JsonlBatch::default(),
            Self::Lines(lines) => JsonlBatch {
                storage: BatchStorage::Lines(lines),
                byte_len,
            },
            Self::Buffer {
                bytes,
                start,
                mut line_ends,
            } => {
                let end = *line_ends.last().expect("buffered batch contains a line");
                for offset in &mut line_ends {
                    *offset -= start;
                }
                JsonlBatch::contiguous(bytes.slice(start..end), line_ends)
            },
        }
    }
}

#[derive(Default)]
struct BatchBuilder {
    storage: BuilderStorage,
    byte_len: usize,
    len: usize,
}

impl BatchBuilder {
    fn len(&self) -> usize {
        self.len
    }
    fn byte_len(&self) -> usize {
        self.byte_len
    }
    fn push(&mut self, line: FramedLine) {
        let previous_bytes = self.byte_len;
        self.byte_len = self
            .byte_len
            .checked_add(line.len())
            .expect("JSONL batch byte length exceeds usize");
        self.len += 1;
        match (&mut self.storage, line) {
            (BuilderStorage::Empty, FramedLine::Buffer { bytes, range }) => {
                self.storage = BuilderStorage::Buffer {
                    bytes,
                    start: range.start,
                    line_ends: alloc::vec![range.end],
                };
            },
            (
                BuilderStorage::Buffer {
                    bytes, line_ends, ..
                },
                FramedLine::Buffer { bytes: next, range },
            ) if bytes.as_ptr() == next.as_ptr()
                && bytes.len() == next.len()
                && line_ends.last().copied() == Some(range.start) =>
            {
                line_ends.push(range.end);
            },
            (BuilderStorage::Lines(lines), line) => lines.push(line.into_line()),
            (_, line) => {
                let storage = core::mem::take(&mut self.storage);
                let mut lines = storage.finish(previous_bytes).into_lines();
                lines.push(line.into_line());
                self.storage = BuilderStorage::Lines(lines);
            },
        }
    }
    fn finish(self) -> JsonlBatch {
        self.storage.finish(self.byte_len)
    }
}

impl From<Vec<JsonlLine>> for JsonlBatch {
    fn from(lines: Vec<JsonlLine>) -> Self {
        Self::new(lines)
    }
}

impl TryFrom<Vec<Vec<u8>>> for JsonlBatch {
    type Error = JsonlLineError;
    /// Validates every raw line. On failure, the error offset refers to the
    /// offending line's bytes rather than the concatenated batch.
    fn try_from(lines: Vec<Vec<u8>>) -> Result<Self, Self::Error> {
        lines.into_iter().map(JsonlLine::owned).collect()
    }
}

impl FromIterator<JsonlLine> for JsonlBatch {
    fn from_iter<T: IntoIterator<Item = JsonlLine>>(iter: T) -> Self {
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

/// Validated [`JsonlLine`] values for framing and line-at-a-time adapters.
pub type LineStream<E = ExecutorError> = Pin<Box<dyn Stream<Item = Result<JsonlLine, E>> + Send>>;

/// Groups complete lines by count, byte target, or elapsed collection time.
///
/// These input lines are already detached values. For a byte reader, use
/// [`crate::jsonl_batches`] to preserve contiguous backing metadata directly
/// through the framer and batch builder instead of detaching and regrouping lines.
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
    source: impl Stream<Item = Result<JsonlLine, E>> + Send + 'static,
    options: BatchOptions,
) -> BatchStream<E> {
    batch_frames(source.map(|line| line.map(FramedLine::Line)), options)
}

/// The common batching policy. Reader provenance is available here, allowing
/// contiguous batches without storing backing metadata in public line values.
pub(crate) fn batch_frames<E: Send + 'static>(
    source: impl Stream<Item = Result<FramedLine, E>> + Send + 'static,
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
            let mut batch = BatchBuilder::default();
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
                yield Ok(batch.finish());
                if let Err(error) = terminal {
                    yield Err(error);
                }
                return;
            }
            yield Ok(batch.finish());
        }
    })
}

/// Adapts batches to individual [`JsonlLine`] values without copying payload bytes.
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

    #[test]
    fn contiguous_and_detached_batches_have_the_same_line_semantics() {
        for (bytes, expected) in [
            (b"".as_slice(), vec![]),
            (b"\n", vec![b"\n".as_slice()]),
            (b"\r\n{}", vec![b"\r\n".as_slice(), b"{}"]),
            (b"a\nb\nc\n", vec![b"a\n".as_slice(), b"b\n", b"c\n"]),
            (b"first\nlast", vec![b"first\n".as_slice(), b"last"]),
        ] {
            let batch = JsonlBatch::from_bytes(Bytes::copy_from_slice(bytes));
            assert_eq!(batch.len(), expected.len());
            assert_eq!(batch.byte_len(), bytes.len());
            assert_eq!(batch.lines().collect::<Vec<_>>(), expected);
            let detached = JsonlBatch::new(batch.clone().into_lines());
            assert_eq!(batch, detached);
            let mut actual = batch.lines();
            let mut expected_iter = expected.iter().copied();
            assert_eq!(actual.next(), expected_iter.next());
            assert_eq!(actual.len(), expected_iter.len());
            assert_eq!(actual.next_back(), expected_iter.next_back());
            assert_eq!(actual.len(), expected_iter.len());
            for expected_line in expected_iter {
                assert_eq!(actual.next(), Some(expected_line));
            }
            assert_eq!(actual.size_hint(), (0, Some(0)));
            assert_eq!(actual.next(), None);
            assert_eq!(actual.next_back(), None);
        }
    }

    #[test]
    fn detaching_lines_does_not_infer_a_contiguous_allocation() {
        let original = JsonlBatch::from_bytes(Bytes::from_static(b"a\nb\n"));
        assert!(original.as_contiguous_bytes().is_some());
        let detached = JsonlBatch::new(original.into_lines());
        assert!(detached.as_contiguous_bytes().is_none());
        let wire: Vec<_> = detached
            .wire_slices(16)
            .unwrap()
            .iter()
            .flat_map(|slice| slice.iter().copied())
            .collect();
        assert_eq!(wire, b"a\nb\n");
    }

    #[test]
    fn raw_batch_construction_rejects_embedded_records() {
        assert_eq!(
            JsonlBatch::try_from(vec![b"{}\n[]".to_vec()]).unwrap_err(),
            JsonlLineError::EmbeddedLf { offset: 2 }
        );
    }

    #[test]
    fn contiguous_views_never_include_filtered_out_lines_or_reorder_records() {
        let backing = Bytes::from_static(b"a\nsecret\nb\n");
        let a = JsonlLine::shared_slice(backing.clone(), 0..2).unwrap();
        let b = JsonlLine::shared_slice(backing, 9..11).unwrap();
        for (lines, expected) in [
            (vec![a.clone(), b.clone()], b"a\nb\n"),
            (vec![b, a], b"b\na\n"),
        ] {
            let batch = JsonlBatch::new(lines);
            assert_eq!(batch.byte_len(), 4);
            assert!(batch.as_contiguous_bytes().is_none());
            let bytes: Vec<_> = batch
                .wire_slices(16)
                .unwrap()
                .iter()
                .flat_map(|slice| slice.iter().copied())
                .collect();
            assert_eq!(bytes, expected);
            let compact = batch.into_compact();
            assert_eq!(compact.as_contiguous_bytes().unwrap(), expected);
        }
    }

    #[test]
    fn compacting_preserves_empty_and_unterminated_line_boundaries() {
        let batch = JsonlBatch::try_from(vec![
            Vec::new(),
            b"{}".to_vec(),
            b"\r\n".to_vec(),
            b"x\n".to_vec(),
        ])
        .unwrap();
        let compact = batch.clone().into_compact();
        assert_eq!(compact, batch);
        assert!(compact.as_contiguous_bytes().is_none());
        let bytes: Vec<_> = compact
            .wire_slices(16)
            .unwrap()
            .iter()
            .flat_map(|slice| slice.iter().copied())
            .collect();
        assert_eq!(bytes, b"\n{}\n\r\nx\n");
    }

    #[test]
    fn compacting_a_sparse_batch_releases_its_backing_owner() {
        struct Owner {
            bytes: Vec<u8>,
            dropped: Arc<AtomicBool>,
        }
        impl AsRef<[u8]> for Owner {
            fn as_ref(&self) -> &[u8] {
                &self.bytes
            }
        }
        impl Drop for Owner {
            fn drop(&mut self) {
                self.dropped.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let mut bytes = vec![b'x'; 256 * 1024];
        bytes[..11].copy_from_slice(b"a\nsecret\nb\n");
        let backing = Bytes::from_owner(Owner {
            bytes,
            dropped: dropped.clone(),
        });
        let batch = JsonlBatch::new(vec![
            JsonlLine::shared_slice(backing.clone(), 0..2).unwrap(),
            JsonlLine::shared_slice(backing, 9..11).unwrap(),
        ]);
        assert!(!dropped.load(Ordering::SeqCst));
        let compact = batch.into_compact();
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(compact.as_contiguous_bytes().unwrap(), b"a\nb\n");
    }

    fn lines(values: &[&[u8]]) -> LineStream<Infallible> {
        Box::pin(crate::stream::iter(
            values
                .iter()
                .map(|line| Ok(JsonlLine::copy_from_slice(line).unwrap()))
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
        let batch =
            JsonlBatch::try_from(vec![b"{}\r\n".to_vec(), Vec::new(), b"tail".to_vec()]).unwrap();
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
            batches
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
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
                batches
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .lines()
                    .collect::<Vec<_>>(),
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
            batches
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            vec![b"first\n".to_vec()]
        );
        assert_eq!(start.elapsed(), Duration::from_millis(10));
    }

    #[tokio::test(start_paused = true)]
    async fn never_emits_empty_batches_and_zero_delay_emits_immediately() {
        let mut pending = batch_lines(
            crate::stream::pending::<Result<JsonlLine, Infallible>>(),
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
            batches
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
            vec![b"{}\n".to_vec()]
        );
        assert_eq!(start.elapsed(), Duration::from_millis(5));
        assert_eq!(
            batches
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
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
        type Item = Result<JsonlLine, &'static str>;
        fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.step += 1;
            Poll::Ready(Some(match self.step {
                1 => Ok(JsonlLine::owned(b"first\n".to_vec()).unwrap()),
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
            batches
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
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
            batches
                .next()
                .await
                .unwrap()
                .unwrap()
                .lines()
                .collect::<Vec<_>>(),
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
            assert_eq!(flattened.next().await.unwrap().unwrap().as_bytes(), line);
        }
        assert!(flattened.next().await.is_none());
    }
}
