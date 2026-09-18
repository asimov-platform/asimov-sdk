// This is free and unencumbered software released into the public domain.

//! Byte and JSONL line sources for a child process's standard input.
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
/// lines; `Input::Jsonl` connects a graph producer's output directly to a consumer.
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
/// reader or line stream. Use `Input::as_stdio` to preserve the source for execution.
#[derive(Debug)]
pub enum Input {
    /// Supplies no bytes by configuring stdin to read from the null device.
    Ignored,
    /// Supplies bytes from an owned asynchronous reader through a stdin pipe.
    ///
    /// The reader must support use across asynchronous tasks (`Send + Sync`)
    /// and unpinned I/O (`Unpin`). Its contents are omitted from debug output.
    AsyncRead(#[debug(skip)] Box<dyn AsyncRead + Send + Sync + Unpin>),
    /// Supplies JSONL lines, applying backpressure and propagating source errors.
    /// Existing line endings are preserved; an LF is appended to any item that
    /// does not end in LF so adjacent records cannot run together.
    /// An empty item therefore writes a blank line. Items are not checked for
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

    /// Adapts byte input to line-based JSONL input without parsing its contents.
    ///
    /// Wraps [`AsyncRead`](Self::AsyncRead) using [`crate::jsonl_lines`]; other
    /// variants are returned unchanged. Adaptation is lazy and performs no I/O.
    /// When fed to a child, a final unterminated line gains an LF as described
    /// by [`Jsonl`](Self::Jsonl).
    #[cfg(feature = "std")]
    pub fn into_jsonl(self) -> Self {
        match self {
            Self::AsyncRead(reader) => Self::Jsonl(crate::jsonl_lines(reader)),
            input => input,
        }
    }

    #[cfg(feature = "std")]
    pub(crate) async fn write_to(
        &mut self,
        stdin: Option<tokio::process::ChildStdin>,
    ) -> Result<(), crate::completion::InputFailure> {
        use crate::completion::InputFailure;
        use futures_lite::StreamExt;
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
            Self::Jsonl(lines) => {
                while let Some(line) = lines.next().await {
                    let line = line.map_err(InputFailure::Source)?;
                    stdin.write_all(&line).await?;
                    if !line.ends_with(b"\n") {
                        stdin.write_all(b"\n").await?;
                    }
                }
            },
        }
        stdin.shutdown().await?;
        Ok(())
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
