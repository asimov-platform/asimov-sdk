// This is free and unencumbered software released into the public domain.

//! Destinations and capture policies for a child process's standard output.
//!
//! The content-specific aliases all refer to [`Output`]; they describe expected
//! payloads without parsing or validating them. [`NoOutput`] represents a program
//! pattern with no output value, rather than a request to discard a stream.

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
    /// Connects the stream to the corresponding standard stream of the parent.
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
