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
/// Stdout handling for a graph producer, whose captured output is a JSONL line stream.
pub type GraphOutput = Output;
/// The absence of an output value for a program pattern.
pub type NoOutput = ();
/// An output stream intended to contain a SPARQL query, such as a compiler's result.
pub type QueryOutput = Output;
/// An output stream intended to contain text, without enforcing an encoding.
pub type TextOutput = Output;

/// How a child's stdout should be connected or collected.
///
/// Most program wrappers return bytes only when stdout is piped, using
/// [`Captured`](Self::Captured) or [`AsyncWrite`](Self::AsyncWrite). Ignored and
/// inherited streams produce no captured bytes. A prompter always captures
/// stdout regardless of this setting.
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
    /// Current program wrappers capture the pipe but do not forward bytes to
    /// this writer. The writer is omitted from debug output.
    AsyncWrite(#[debug(skip)] Box<dyn AsyncWrite + Send + Sync + Unpin>),
}

impl Output {
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
