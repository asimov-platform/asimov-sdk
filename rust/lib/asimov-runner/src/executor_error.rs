// This is free and unencumbered software released into the public domain.

//! Results and error categories for child-process execution.
//!
//! [`ExecutorError`] distinguishes failures to start a program from unsuccessful
//! exits and other I/O failures. Recognized sysexits statuses are exposed as
//! [`SysexitsError`] values so callers can handle them without parsing messages.

use crate::SysexitsError;
use alloc::{string::String, vec::Vec};
use core::fmt;
use std::{ffi::OsString, io::Cursor};

/// Captured stdout from a successful process, or an execution failure.
///
/// The cursor contains raw bytes, is positioned at zero, and is empty when stdout
/// was not captured. The complete output is buffered before this result is
/// returned; it is not a live stream from the child.
pub type ExecutorResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>;

/// A failure to launch, communicate with, or successfully complete a child process.
///
/// Conversion from a process output decodes stderr strictly as UTF-8: invalid
/// UTF-8 yields `None`, while empty stderr yields `Some(String::new())`.
/// Conversion from an exit status alone has no stderr and always uses `None`.
#[derive(Debug)]
pub enum ExecutorError {
    /// Spawning returned `NotFound`; contains the selected program name or path.
    MissingProgram(OsString),
    /// The process could not be started for a reason other than `NotFound`.
    SpawnFailure(std::io::Error),
    /// A recognized sysexits error and optional captured UTF-8 stderr.
    Failure(SysexitsError, Option<String>),
    /// An unrecognized exit code and optional captured UTF-8 stderr.
    ///
    /// The code is `None` when termination has no numeric exit code, such as
    /// termination by a signal on Unix.
    UnexpectedFailure(Option<i32>, Option<String>),
    /// An I/O failure outside spawning, such as copying stdin, waiting for the
    /// child, or decoding a prompter's stdout as UTF-8.
    UnexpectedOther(std::io::Error),
}

impl core::error::Error for ExecutorError {}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProgram(program) => {
                write!(f, "Missing program: {}", program.to_string_lossy())
            },
            Self::SpawnFailure(err) => write!(f, "Failed to spawn process: {}", err),
            Self::Failure(error, stderr) => {
                write!(
                    f,
                    "Command failed with exit code {}",
                    error.code().unwrap_or(-1),
                )?;
                if let Some(stderr) = stderr {
                    write!(f, "\n{}", stderr)?;
                }
                Ok(())
            },
            Self::UnexpectedFailure(code, stderr) => {
                write!(
                    f,
                    "Command failed with unexpected exit code: {}",
                    code.unwrap_or(-1)
                )?;
                if let Some(stderr) = stderr {
                    write!(f, "\n{}", stderr)?;
                }
                Ok(())
            },
            Self::UnexpectedOther(err) => write!(f, "Unexpected error: {}", err),
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for ExecutorError {
    fn from(error: std::io::Error) -> Self {
        Self::UnexpectedOther(error)
    }
}

#[cfg(feature = "std")]
impl From<std::process::ExitStatus> for ExecutorError {
    fn from(status: std::process::ExitStatus) -> Self {
        match SysexitsError::try_from(status) {
            Ok(error) => Self::Failure(error, None),
            Err(code) => Self::UnexpectedFailure(code, None),
        }
    }
}

#[cfg(feature = "std")]
impl From<std::process::Output> for ExecutorError {
    fn from(output: std::process::Output) -> Self {
        let stderr = String::from_utf8(output.stderr).ok();
        match SysexitsError::try_from(output.status) {
            Ok(error) => Self::Failure(error, stderr),
            Err(code) => Self::UnexpectedFailure(code, stderr),
        }
    }
}
