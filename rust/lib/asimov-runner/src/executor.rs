// This is free and unencumbered software released into the public domain.

//! Low-level Tokio child-process execution with ASIMOV executable lookup.
//!
//! [`Executor`] configures a command without starting it. Call
//! [`Executor::execute`] for a complete spawn-and-wait cycle, or use
//! [`Executor::spawn`] and [`Executor::wait`] to interact with the child between
//! those steps. `execute` and `wait` buffer captured output until the child exits;
//! `spawn` returns the live child handle without collecting its output.
//! [`Executor::execute_jsonl`] and [`Executor::execute_jsonl_with_input`] instead
//! return live line streams with concurrent input/output handling.

use crate::{Command, ExecutorError, ExecutorResult, Input};
use alloc::borrow::ToOwned;
use std::{
    ffi::OsStr,
    io::{Cursor, ErrorKind},
    process::Stdio,
};
use tokio::process::Child;

/// A configured command that reports process failures as [`ExecutorError`].
///
/// Executables are looked up in the ASIMOV root's `libexec` directory before
/// falling back to the supplied program name or path. By default, all three
/// standard streams are connected to the null device, `NO_COLOR=1` is set, and
/// spawned children are configured to be killed when their handles are dropped.
/// Use [`command`](Self::command) to customize these settings before execution.
///
/// Each execution spawns a new process using the stored command configuration.
/// Stdout is returned only when configured as a pipe; otherwise the successful
/// result is an empty cursor or stream. Captured stderr is included in process
/// exit errors when it is valid UTF-8 and is discarded on success. An input or
/// I/O error may be reported before an exit error, without captured diagnostics.
///
/// # Example
///
/// Prepare a piped input and capture both output streams before spawning:
///
/// ```no_run
/// use asimov_runner::{Executor, Input};
/// use std::io::Cursor;
///
/// # async fn example() -> Result<(), asimov_runner::ExecutorError> {
/// let mut input = Input::AsyncRead(Box::new(Cursor::new(b"example input".to_vec())));
/// let mut executor = Executor::new("asimov-example-reader");
/// executor.command().stdin(input.as_stdio());
/// executor.capture_stdout();
/// executor.capture_stderr();
///
/// let bytes = executor.execute_with_input(&mut input).await?.into_inner();
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Executor(Command);

impl Executor {
    /// Prepares a command with the executor's default environment and streams.
    ///
    /// If `<asimov_root>/libexec/<program>` exists, it is selected; otherwise
    /// `program` is passed to Tokio unchanged, allowing normal `PATH` lookup for
    /// a bare name. Existence does not guarantee that the selected file can be
    /// executed. Such errors are reported when the command is spawned.
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        let libexec_path = asimov_env::paths::asimov_root()
            .join("libexec")
            .join(program.as_ref());

        let mut command = if libexec_path.exists() {
            Command::new(libexec_path)
        } else {
            Command::new(program)
        };

        command.env("NO_COLOR", "1"); // See: https://no-color.org
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.kill_on_drop(true);
        Self(command)
    }

    /// Returns the underlying command for configuring arguments, environment,
    /// working directory, standard streams, or process-lifetime behavior.
    pub fn command(&mut self) -> &mut Command {
        &mut self.0
    }

    /// Connects the child's stdin to the null device, giving it immediate EOF.
    pub fn ignore_stdin(&mut self) {
        self.0.stdin(Stdio::null());
    }

    /// Discards the child's stdout by connecting it to the null device.
    pub fn ignore_stdout(&mut self) {
        self.0.stdout(Stdio::null());
    }

    /// Discards the child's stderr, leaving no diagnostic text to capture.
    pub fn ignore_stderr(&mut self) {
        self.0.stderr(Stdio::null());
    }

    /// Pipes stdout for buffered execution or live JSONL streaming.
    pub fn capture_stdout(&mut self) {
        self.0.stdout(Stdio::piped());
    }

    /// Pipes stderr so execution can attach its UTF-8 contents to exit errors.
    pub fn capture_stderr(&mut self) {
        self.0.stderr(Stdio::piped());
    }

    /// Spawns the command and waits for completion, returning captured stdout.
    ///
    /// No input is written by this method. Use
    /// [`execute_with_input`](Self::execute_with_input) to copy an input stream
    /// into the child.
    ///
    /// # Errors
    ///
    /// Returns an error if spawning or waiting fails, or if the child exits
    /// unsuccessfully. See [`ExecutorError`] for the error categories.
    pub async fn execute(&mut self) -> ExecutorResult {
        let process = self.spawn().await?;
        self.wait(process).await
    }

    /// Spawns the command, copies `input` into stdin, and waits for completion.
    ///
    /// Configure stdin with `input.as_stdio()` through [`command`](Self::command)
    /// before calling this method. [`Input::Ignored`] performs no copy and does
    /// not change the command's stdin configuration. An asynchronous reader is
    /// normally consumed from its current position to EOF, then the stdin pipe
    /// is closed. Reusing the input does not rewind it or restore partly written
    /// bytes after early exit or cancellation.
    ///
    /// Input is fed concurrently with draining stdout and stderr. JSONL input
    /// is written one line at a time with backpressure and source error propagation.
    /// Once process completion and output collection are observed, any pending
    /// input feed is cancelled. A successful result does not prove the entire
    /// input was consumed; unread source errors may not have been observed.
    ///
    /// # Errors
    ///
    /// Returns an error if spawning, copying input, or waiting fails, or if the
    /// child exits unsuccessfully. Non-ignored input requires piped stdin;
    /// otherwise feeding it returns an I/O `InvalidInput` error.
    pub async fn execute_with_input(&mut self, input: &mut Input) -> ExecutorResult {
        let process = self.spawn().await?;
        let output = communicate(process, input).await?;
        if !output.status.success() {
            return Err(output.into());
        }
        Ok(Cursor::new(output.stdout))
    }

    /// Starts a new child process using the current command configuration.
    ///
    /// The returned handle provides access to any piped standard streams. Unless
    /// overridden through [`command`](Self::command), dropping that handle before
    /// completion requests termination of that child, without waiting for it to
    /// be reaped or guaranteeing termination of its descendants.
    ///
    /// # Errors
    ///
    /// Maps an I/O `NotFound` error to [`ExecutorError::MissingProgram`] and all
    /// other spawn errors to [`ExecutorError::SpawnFailure`].
    pub async fn spawn(&mut self) -> Result<Child, ExecutorError> {
        match self.0.spawn() {
            Ok(process) => Ok(process),
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let program = self.0.as_std().get_program().to_owned();
                return Err(ExecutorError::MissingProgram(program));
            },
            Err(err) => return Err(ExecutorError::SpawnFailure(err)),
        }
    }

    /// Collects the child's remaining piped output and waits for it to exit.
    ///
    /// Any stdin handle still owned by `process` is closed before waiting.
    /// On success, returns all captured stdout in a cursor positioned at zero.
    /// Streams whose handles were taken from the child are not collected here.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutorError::UnexpectedOther`] for I/O errors. An unsuccessful
    /// exit becomes [`ExecutorError::Failure`] for a recognized sysexits status
    /// or [`ExecutorError::UnexpectedFailure`] otherwise, with captured UTF-8
    /// stderr attached. Stdout is not retained in either failure variant.
    pub async fn wait(&mut self, process: Child) -> ExecutorResult {
        let output = process.wait_with_output().await?;

        #[cfg(feature = "tracing")]
        tracing::trace!("The command exited with: {}", output.status);

        if !output.status.success() {
            return Err(output.into());
        }

        Ok(Cursor::new(output.stdout))
    }
}

/// Feeds stdin while draining the remaining pipes. When exit and pipe collection
/// complete before the input feed, cancel that feed rather than await its source.
pub(crate) async fn communicate(
    mut process: Child,
    input: &mut Input,
) -> Result<std::process::Output, ExecutorError> {
    let feed = input.write_to(process.stdin.take());
    let completion = process.wait_with_output();
    tokio::pin!(feed, completion);
    tokio::select! {
        result = &mut completion => Ok(result?),
        result = &mut feed => {
            result?;
            Ok(completion.await?)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_success() {
        let mut runner = Executor::new("curl");
        runner.command().arg("https://www.google.com");
        let result = runner.execute().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_missing_program() {
        let mut runner = Executor::new("this-command-does-not-exist");
        let result = runner.execute().await;
        assert!(matches!(result, Err(ExecutorError::MissingProgram(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_spawn_failure() {
        let mut runner = Executor::new("/dev/null");
        let result = runner.execute().await;
        assert!(matches!(result, Err(ExecutorError::SpawnFailure(_))));
    }

    #[tokio::test]
    async fn test_unexpected_failure() {
        let mut runner = Executor::new("curl");
        let result = runner.execute().await;
        assert!(matches!(
            result,
            Err(ExecutorError::UnexpectedFailure(_, _))
        ));
    }
}
