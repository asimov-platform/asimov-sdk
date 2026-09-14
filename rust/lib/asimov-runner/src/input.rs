// This is free and unencumbered software released into the public domain.

//! Byte sources for a child process's standard input.
//!
//! The content-specific aliases all refer to [`Input`]; they express a program
//! pattern's expected payload without imposing an encoding or validating bytes.
//! [`NoInput`] instead represents a pattern that has no input parameter.

use alloc::boxed::Box;
use derive_more::Debug;
use tokio::io::AsyncRead;

/// An input stream with no prescribed content type.
pub type AnyInput = Input;
/// An input stream intended to contain a serialized RDF graph or dataset.
pub type GraphInput = Input;
/// The absence of an input value for a program pattern.
pub type NoInput = ();
/// An input stream intended to contain a SPARQL query.
pub type QueryInput = Input;
/// An input stream intended to contain text, without enforcing an encoding.
pub type TextInput = Input;

/// The source of bytes to supply to a child's stdin.
///
/// Program wrappers configure the child's stdin from this value and copy a
/// reader to the resulting pipe during execution. A reader is owned by the
/// input and consumed from its current position; repeated executions do not
/// replay bytes that have already been read.
///
/// With `std` enabled, conversion to `Stdio` only selects null or piped stdin;
/// it does not transfer bytes. The consuming conversion also drops any stored
/// reader. Use `Input::as_stdio` to preserve the reader for execution.
#[derive(Debug)]
pub enum Input {
    /// Supplies no bytes by configuring stdin to read from the null device.
    Ignored,
    /// Supplies bytes from an owned asynchronous reader through a stdin pipe.
    ///
    /// The reader must support use across asynchronous tasks (`Send + Sync`)
    /// and unpinned I/O (`Unpin`). Its contents are omitted from debug output.
    AsyncRead(#[debug(skip)] Box<dyn AsyncRead + Send + Sync + Unpin>),
}

impl Input {
    /// Selects the child's stdin configuration without consuming this input.
    ///
    /// Returns null stdin for [`Ignored`](Self::Ignored) and a pipe for
    /// [`AsyncRead`](Self::AsyncRead). The caller must still copy the reader's
    /// bytes into the child's pipe after spawning it.
    #[cfg(feature = "std")]
    pub fn as_stdio(&self) -> std::process::Stdio {
        use std::process::Stdio;
        match self {
            Input::Ignored => Stdio::null(),
            Input::AsyncRead(_) => Stdio::piped(),
        }
    }
}

#[cfg(feature = "std")]
impl Into<std::process::Stdio> for Input {
    fn into(self) -> std::process::Stdio {
        use std::process::Stdio;
        match self {
            Input::Ignored => Stdio::null(),
            Input::AsyncRead(_) => Stdio::piped(),
        }
    }
}
