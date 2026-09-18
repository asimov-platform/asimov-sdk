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

use crate::{
    Command, ExecutionCompletion, ExecutorError, ExecutorResult, Input, InputCompletion, Output,
};
use alloc::borrow::ToOwned;
use std::{ffi::OsStr, io::ErrorKind, process::Stdio};
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
/// exit errors when it is valid UTF-8. [`ExecutionCompletion`] exposes exit status,
/// input delivery, and diagnostics separately; its `into_result` method defines
/// the error precedence used by the convenience APIs.
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
    /// Early child exit cancels a pending input feed and is reported as
    /// [`ExecutorError::IncompleteInput`] on an otherwise successful exit. Use
    /// [`execute_with_io_completion`](Self::execute_with_io_completion) to inspect
    /// intentional early exit. Success confirms EOF and delivery to the pipe,
    /// not application-level processing of the supplied data.
    ///
    /// # Errors
    ///
    /// Returns an error if spawning, copying input, or waiting fails, or if the
    /// child exits unsuccessfully. Non-ignored input requires piped stdin;
    /// otherwise feeding it returns an I/O `InvalidInput` error.
    pub async fn execute_with_input(&mut self, input: &mut Input) -> ExecutorResult {
        self.execute_with_io(input, &mut Output::Captured).await
    }

    /// Feeds input and routes stdout concurrently, requiring complete delivery
    /// and a successful exit. Configure stdin/stdout from `input.as_stdio()` and
    /// `output.as_stdio()` first. Only [`Output::Captured`] returns payload bytes.
    ///
    /// # Errors
    ///
    /// Returns spawn, transport, forwarding, input-delivery, or exit errors using
    /// the precedence documented by [`ExecutionCompletion::into_result`].
    pub async fn execute_with_io(
        &mut self,
        input: &mut Input,
        output: &mut Output,
    ) -> ExecutorResult {
        self.execute_with_io_completion(input, output)
            .await?
            .into_result()
    }

    /// Executes with explicit exit and input-delivery outcomes.
    ///
    /// Configure stdin/stdout from the supplied input/output policies before
    /// calling. Input and writers are borrowed and retained for reuse, without
    /// rewinding. Stdout forwarding and stderr collection run concurrently with
    /// feeding stdin and waiting for exit. Early exit cancels pending input;
    /// source errors terminate the child, while broken pipes allow it to finish
    /// naturally so its exit diagnostics are preserved. Ready input outcomes are
    /// polled before exit to give EOF and source errors consistent precedence.
    ///
    /// # Errors
    ///
    /// Only spawn, stdout/stderr transport, writer, and wait failures are returned
    /// directly. Child exit and input failures are fields of the returned
    /// [`ExecutionCompletion`], even when the exit status is unsuccessful.
    pub async fn execute_with_io_completion(
        &mut self,
        input: &mut Input,
        output: &mut Output,
    ) -> Result<ExecutionCompletion, ExecutorError> {
        communicate(self.spawn().await?, input, output).await
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
        communicate(process, &mut Input::Ignored, &mut Output::Captured)
            .await?
            .into_result()
    }
}

/// Supervises input and process exit independently from draining stdout/stderr.
pub(crate) async fn communicate(
    mut process: Child,
    input: &mut Input,
    output: &mut Output,
) -> Result<ExecutionCompletion, ExecutorError> {
    use crate::completion::InputFailure;
    use alloc::vec::Vec;
    use tokio::io::AsyncReadExt;

    let stdin = process.stdin.take();
    let stdout = process.stdout.take();
    let stderr = process.stderr.take();
    let supervise = async {
        let feed = input.write_to(stdin);
        tokio::pin!(feed);
        let input = tokio::select! {
            biased;
            result = &mut feed => match result {
                Ok(()) => InputCompletion::Complete,
                Err(InputFailure::Source(error)) => {
                    process.start_kill()?;
                    InputCompletion::SourceFailed(error)
                },
                Err(InputFailure::Write(error)) => {
                    if error.kind() != ErrorKind::BrokenPipe {
                        process.start_kill()?;
                    }
                    InputCompletion::WriteFailed(error)
                },
            },
            status = process.wait() => {
                return Ok::<_, std::io::Error>((status?, InputCompletion::Interrupted));
            },
        };
        Ok((process.wait().await?, input))
    };
    let read_stderr = async {
        let mut bytes = Vec::new();
        if let Some(mut stderr) = stderr {
            stderr.read_to_end(&mut bytes).await?;
        }
        Ok::<_, std::io::Error>(bytes)
    };
    let result = tokio::try_join!(supervise, output.read_from(stdout), read_stderr);
    match result {
        Ok(((status, input), stdout, stderr)) => {
            #[cfg(feature = "tracing")]
            tracing::trace!("The command exited with: {}", status);
            Ok(ExecutionCompletion {
                output: std::process::Output {
                    status,
                    stdout,
                    stderr,
                },
                input,
            })
        },
        Err(error) => {
            // A failed destination must not leave the producer blocked on its pipes.
            let _ = process.start_kill();
            let _ = process.wait().await;
            Err(error.into())
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
