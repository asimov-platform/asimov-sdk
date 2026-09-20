// This is free and unencumbered software released into the public domain.

//! Shared batching primitives with local-execution error defaults.
//!
//! # An intermediate batch filter
//!
//! This example connects **Fetcher → Rust filter → Writer**. It assumes one
//! JSON-LD object per line with expanded type IRIs; it does not perform JSON-LD
//! context expansion. Retained lines keep their bytes and order. Batch boundaries
//! are transport groupings, not RDF graphs or transactions.
//!
//! Unlike native [`crate::Pipeline::pipe`] connections, an in-process filter is
//! passed to the next program as [`crate::GraphInput::Jsonl`]. Pulling batches
//! supplies backpressure, and source errors are propagated to the consumer.
//!
//! ```no_run
//! use asimov_runner::{
//!     AnyOutput, ExecutorError, Fetcher, GraphInput, GraphOutput, JsonlBatch,
//!     JsonlStream, StreamExt, Writer, stream,
//! };
//! use serde_json::Value;
//! use std::io;
//!
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
//!         if keep { kept.push(line); }
//!     }
//!     Ok(JsonlBatch::new(kept))
//! }
//!
//! fn people_only(mut source: JsonlStream) -> JsonlStream {
//!     Box::pin(stream! {
//!         while let Some(batch) = source.next().await {
//!             match batch.and_then(keep_people) {
//!                 Ok(batch) if !batch.is_empty() => yield Ok(batch),
//!                 Ok(_) => {},
//!                 Err(error) => {
//!                     drop(source);
//!                     yield Err(error);
//!                     return;
//!                 },
//!             }
//!         }
//!     })
//! }
//!
//! # async fn example() -> Result<(), ExecutorError> {
//! let source = Fetcher::new(
//!     "asimov-example-fetcher", "https://example.com/collection",
//!     GraphOutput::Captured, Default::default(),
//! ).execute().await?;
//! let exported = Writer::new(
//!     "asimov-example-writer", GraphInput::Jsonl(people_only(source)),
//!     AnyOutput::Captured, Default::default(),
//! ).execute().await?.into_inner();
//! # Ok(())
//! # }
//! ```

pub(crate) use asimov_flow::batch::{FramedLine, batch_frames};
pub use asimov_flow::{BatchOptions, JsonlBatch, batch_lines, flatten_batches};

pub type BatchStream<E = crate::ExecutorError> = asimov_flow::BatchStream<E>;
pub type LineStream<E = crate::ExecutorError> = asimov_flow::LineStream<E>;
pub(crate) type FrameStream<E = crate::ExecutorError> = asimov_flow::batch::FrameStream<E>;

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
