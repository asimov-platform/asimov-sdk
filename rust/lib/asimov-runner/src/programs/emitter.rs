// This is free and unencumbered software released into the public domain.

//! RDF value generation through an external emitter program with no stdin input.

use crate::{Executor, ExecutorError, GraphOutput, NoInput, Output};
use alloc::{boxed::Box, format, vec, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::EmitterOptions;

/// Raw graph bytes captured from a successful [`Emitter`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// The graph is not parsed or validated.
pub type EmitterResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [emitter] that generates values as RDF without reading stdin.
///
/// Stdin is connected to the null device. The program determines what data to
/// emit from its arguments, environment, and other external sources. Execution
/// uses the buffering and stream-handling behavior described in [`crate::programs`].
///
/// [emitter]: https://asimov-specs.github.io/program-patterns/#emitter
#[allow(unused)]
#[derive(Debug)]
pub struct Emitter {
    executor: Executor,
    options: EmitterOptions,
    input: NoInput,
    output: GraphOutput,
}

impl Emitter {
    /// Configures an emitter without starting it.
    ///
    /// Adds `--output=<format>` when `options.output` is set, followed by
    /// `options.other`. `output` selects stdout handling; stderr is captured
    /// for failure diagnostics.
    pub fn new(program: impl AsRef<OsStr>, output: Output, options: EmitterOptions) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(ref output) = options.output {
                vec![format!("--output={}", output)]
            } else {
                vec![]
            })
            .args(&options.other)
            .stdin(Stdio::null())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input: (),
            output,
        }
    }

    /// Runs a new emitter process and returns its captured graph bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning or waiting fails, or if the
    /// emitter exits unsuccessfully.
    pub async fn execute(&mut self) -> EmitterResult {
        let stdout = self.executor.execute().await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Emitter<Cursor<Vec<u8>>, ExecutorError> for Emitter {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Emitter {
    async fn execute(&mut self) -> EmitterResult {
        self.execute().await
    }
}

#[cfg(test)]
mod tests {
    //use super::*;
    //use asimov_patterns::Execute;

    #[tokio::test]
    async fn test_execute() {
        // TODO
    }
}
