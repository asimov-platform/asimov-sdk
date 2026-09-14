// This is free and unencumbered software released into the public domain.

//! Low-level Tokio child-process execution with ASIMOV executable lookup.
//!
//! [`Executor`] configures a command without starting it. Call
//! [`Executor::execute`] for a complete spawn-and-wait cycle, or use
//! [`Executor::spawn`] and [`Executor::wait`] to interact with the child between
//! those steps. Captured output is buffered in memory until the child exits.

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
/// result is an empty cursor. Captured stderr is included in process-failure
/// errors when it is valid UTF-8 and is discarded on success.
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

    /// Pipes stdout so that [`wait`](Self::wait) can return its bytes.
    pub fn capture_stdout(&mut self) {
        self.0.stdout(Stdio::piped());
    }

    /// Pipes stderr so that [`wait`](Self::wait) can attach it to failure errors.
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
    /// consumed from its current position to EOF, then the stdin pipe is closed.
    /// Reusing the input does not rewind it.
    ///
    /// Input is copied before stdout and stderr are drained. A child that fills
    /// either output pipe before consuming its input can therefore deadlock.
    /// Use separate input/output tasks with [`spawn`](Self::spawn) when the
    /// protocol requires concurrent reads and writes.
    ///
    /// # Errors
    ///
    /// Returns an error if spawning, copying input, or waiting fails, or if the
    /// child exits unsuccessfully.
    ///
    /// # Panics
    ///
    /// Panics for [`Input::AsyncRead`] if the command's stdin is not piped.
    pub async fn execute_with_input(&mut self, input: &mut Input) -> ExecutorResult {
        let mut process = self.spawn().await?;
        match input {
            Input::Ignored => {},
            Input::AsyncRead(reader) => {
                let mut stdin = process.stdin.take().expect("should capture stdin");
                tokio::io::copy(&mut *reader, &mut stdin).await?;
            },
        }
        self.wait(process).await
    }

    /// Starts a new child process using the current command configuration.
    ///
    /// The returned handle provides access to any piped standard streams. Unless
    /// overridden through [`command`](Self::command), dropping that handle before
    /// completion kills the child.
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
