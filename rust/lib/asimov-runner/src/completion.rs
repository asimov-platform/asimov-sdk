// This is free and unencumbered software released into the public domain.

//! Explicit process-exit and input-delivery outcomes.

use crate::{ExecutorError, ExecutorResult};
use std::{io, io::Cursor, process::Output};

/// What happened while supplying a child's stdin.
#[derive(Debug)]
pub enum InputCompletion {
    /// The source reached EOF and all bytes were written, or input was ignored.
    /// This confirms delivery to the pipe, not application-level processing.
    Complete,
    /// The child exited while its input feed was still pending. The source was
    /// cancelled, so unread data and upstream errors may remain unobserved.
    Interrupted,
    /// Reading the source failed. The child was terminated and reaped.
    SourceFailed(ExecutorError),
    /// Writing or closing stdin failed. Broken pipes allow the child to finish
    /// naturally so its exit diagnostics can be collected; other errors terminate it.
    WriteFailed(io::Error),
}

/// A reaped child's output and input-delivery outcome, including unsuccessful exits.
///
/// Returned by [`crate::Executor::execute_with_io_completion`]. Inspect both
/// fields when early child exit is intentional. Capture/forwarding and wait
/// errors are returned directly instead of producing a completion value.
#[derive(Debug)]
pub struct ExecutionCompletion {
    /// Exit status, captured stderr, and captured stdout. Forwarded stdout is empty.
    pub output: Output,
    /// Whether the input source finished and all its bytes reached the stdin pipe.
    pub input: InputCompletion,
}

impl ExecutionCompletion {
    /// Requires a successful exit and complete input delivery, returning stdout.
    ///
    /// Error precedence is: source errors and non-broken-pipe stdin errors
    /// (which cause termination), unsuccessful child exit with stderr, then
    /// broken-pipe errors or [`ExecutorError::IncompleteInput`]. Thus an exit
    /// failure is not hidden by a downstream broken pipe, and an upstream error
    /// is not hidden by the termination it caused. Ready input outcomes are
    /// polled before child exit; pending sources are never drained after exit.
    pub fn into_result(self) -> ExecutorResult {
        match self.input {
            InputCompletion::SourceFailed(error) => return Err(error),
            InputCompletion::WriteFailed(error) if error.kind() != io::ErrorKind::BrokenPipe => {
                return Err(error.into());
            },
            input => {
                if !self.output.status.success() {
                    return Err(self.output.into());
                }
                match input {
                    InputCompletion::Complete => {},
                    InputCompletion::Interrupted => return Err(ExecutorError::IncompleteInput),
                    InputCompletion::WriteFailed(error) => return Err(error.into()),
                    InputCompletion::SourceFailed(_) => unreachable!(),
                }
            },
        }
        Ok(Cursor::new(self.output.stdout))
    }
}

pub(crate) enum InputFailure {
    Source(ExecutorError),
    Write(io::Error),
}

impl From<io::Error> for InputFailure {
    fn from(error: io::Error) -> Self {
        Self::Write(error)
    }
}
