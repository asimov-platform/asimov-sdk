// This is free and unencumbered software released into the public domain.

//! Destinations and capture policies for a child process's standard output.
//!
//! The content-specific aliases all refer to [`Output`]; they describe expected
//! payloads without parsing or validating them. [`NoOutput`] represents a program
//! pattern with no output value, rather than a request to discard a stream.
//!
//! # Inherited graph output and error handling
//!
//! `GraphOutput::Inherited` normally connects the child's stdout directly to the
//! parent's stdout, including any redirection or pipe attached to it. The runner
//! does not capture, decode, or batch those bytes. Output `BatchOptions` therefore
//! have no effect on inherited stdout, and do not control the child's own buffering.
//! Program format options (such as `FetcherOptions::output`) still select the
//! serialization. For graph consumers, batching of their input remains applicable.
//!
//! Graph-producing wrappers still return a stream. With inherited stdout, it
//! yields no successful payload batches: it ends on successful completion or
//! yields an error. **Consume that stream to completion**, even though stdout is
//! already visible. Polling drives stdin feeding, captured stderr draining, and
//! process waiting. The outer `execute().await?` handles startup errors; the
//! inner `result?` handles later input, I/O, and exit failures. Dropping the stream
//! immediately after spawning instead requests child termination. Output already
//! written to stdout cannot be withdrawn if the child subsequently fails.
//! Since the runner does not perform directly inherited stdout writes, failures
//! of those writes must be reported by the child, typically through its exit status.
//!
//! ```no_run
//! # #[cfg(feature = "std")]
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> Result<(), asimov_runner::ExecutorError> {
//!     use asimov_runner::{Fetcher, FetcherOptions, GraphOutput};
//!     use futures_lite::StreamExt;
//!
//!     let mut fetcher = Fetcher::new(
//!         "asimov-example-fetcher",
//!         "https://example.com/resource",
//!         GraphOutput::Inherited,
//!         FetcherOptions::builder().output("jsonl").build(),
//!     );
//!
//!     let mut execution = fetcher.execute().await?;
//!     while let Some(result) = execution.next().await {
//!         result?; // Propagate failures; there are no captured batches to print.
//!     }
//!     Ok(()) // Completion, not merely successful spawning, has been checked.
//! }
//! # #[cfg(not(feature = "std"))]
//! # fn main() {}
//! ```
//!
//! The same consumption loop applies to a graph-producing pipeline with an
//! inherited-output tail; its stream reports `PipelineError` and checks all stages.
//! A limited `Lister` is the routing exception: it pipes, counts, and forwards
//! stdout to the parent so the local line cap cannot be bypassed. It still emits
//! no payload batches, and polling is required to perform that forwarding. Reaching
//! the cap deliberately stops the child without checking its eventual exit status;
//! a zero limit starts no child. See the [lister execution contract][lister].
//!
//! [lister]: https://docs.rs/asimov-runner/latest/asimov_runner/struct.Lister.html#method.execute

use alloc::boxed::Box;
use derive_more::Debug;
use tokio::io::AsyncWrite;

/// An output stream with no prescribed content type.
pub type AnyOutput = Output;
/// Stdout handling for a graph producer, whose captured output is a JSONL batch stream.
pub type GraphOutput = Output;
/// The absence of an output value for a program pattern.
pub type NoOutput = ();
/// An output stream intended to contain a SPARQL query, such as a compiler's result.
pub type QueryOutput = Output;
/// An output stream intended to contain text, without enforcing an encoding.
pub type TextOutput = Output;

/// How a child's stdout should be connected or collected.
///
/// Only [`Captured`](Self::Captured) returns payload bytes. Ignored, inherited,
/// and forwarded output produces an empty result payload. Execution still checks
/// input delivery and process success for every mode.
///
/// With `std` enabled, conversion to `Stdio` only configures the stream; it does
/// not copy bytes into a writer. The consuming conversion drops any stored
/// writer, while `Output::as_stdio` preserves it.
#[derive(Debug)]
pub enum Output {
    /// Discards output by connecting the stream to the null device.
    Ignored,
    /// Connects stdout to the parent's stdout, with no captured output batches.
    /// Output batching settings do not apply. Consume graph execution streams
    /// to completion to observe failures; see the [module example](self).
    /// A limited lister routes through a line-counting pipe before forwarding.
    Inherited,
    /// Pipes output for streaming or collection into the program's result.
    Captured,
    /// Stores an asynchronous destination and requests a pipe for the child.
    ///
    /// Wrappers forward stdout incrementally with backpressure and flush the
    /// writer at EOF, without shutting it down. No bytes are also captured.
    /// Write/flush failures fail execution. Graph streams own the writer after
    /// successful spawning; subsequent calls on that wrapper discard stdout.
    /// Buffered wrappers retain the writer for reuse. Omitted from debug output.
    AsyncWrite(#[debug(skip)] Box<dyn AsyncWrite + Send + Sync + Unpin>),
}

impl Output {
    #[cfg(feature = "std")]
    pub(crate) fn take_for_stream(&mut self) -> Self {
        match self {
            Self::Ignored => Self::Ignored,
            Self::Inherited => Self::Inherited,
            Self::Captured => Self::Captured,
            Self::AsyncWrite(_) => core::mem::replace(self, Self::Ignored),
        }
    }

    #[cfg(feature = "std")]
    pub(crate) async fn read_from(
        &mut self,
        stdout: Option<tokio::process::ChildStdout>,
    ) -> std::io::Result<alloc::vec::Vec<u8>> {
        use alloc::vec::Vec;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut captured = Vec::new();
        if let Some(mut stdout) = stdout {
            match self {
                Self::Captured => {
                    stdout.read_to_end(&mut captured).await?;
                },
                Self::AsyncWrite(writer) => {
                    tokio::io::copy(&mut stdout, writer).await?;
                    writer.flush().await?;
                },
                Self::Ignored | Self::Inherited => {
                    tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await?;
                },
            }
        }
        Ok(captured)
    }

    /// Selects the child's stream configuration without consuming this value.
    ///
    /// Both [`Captured`](Self::Captured) and [`AsyncWrite`](Self::AsyncWrite)
    /// request a pipe. Reading that pipe and forwarding bytes, if desired, are
    /// separate operations that this method does not perform.
    #[cfg(feature = "std")]
    pub fn as_stdio(&self) -> std::process::Stdio {
        use std::process::Stdio;
        match self {
            Output::Ignored => Stdio::null(),
            Output::Inherited => Stdio::inherit(),
            Output::Captured => Stdio::piped(),
            Output::AsyncWrite(_) => Stdio::piped(),
        }
    }
}

#[cfg(feature = "std")]
impl Into<std::process::Stdio> for Output {
    fn into(self) -> std::process::Stdio {
        use std::process::Stdio;
        match self {
            Output::Ignored => Stdio::null(),
            Output::Inherited => Stdio::inherit(),
            Output::Captured => Stdio::piped(),
            Output::AsyncWrite(_) => Stdio::piped(),
        }
    }
}
