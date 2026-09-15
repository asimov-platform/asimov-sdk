// This is free and unencumbered software released into the public domain.

//! Language runtime execution with named definitions and program input.

use crate::{Executor, ExecutorError, Input, Output};
use alloc::{boxed::Box, format, vec::Vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::RunnerOptions;

/// Raw stdout bytes captured from a successful [`Runner`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// The pattern specifies a text execution result; this wrapper returns its raw
/// bytes without decoding or enforcing an encoding.
pub type RunnerResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [runner] that executes program text in a language runtime.
///
/// The pattern consumes text conforming to the runtime's grammar and produces
/// the execution result as text. This wrapper transports input and output as
/// bytes; the external program parses and executes the input.
///
/// Each definition is passed as a `--define=<key>=<value>` argument; its meaning
/// is determined by the runtime. Input and output use the buffering and
/// stream-handling behavior described in [`crate::programs`]. For direct
/// control over command configuration, use [`Executor`] instead.
///
/// [runner]: https://asimov-specs.github.io/program-patterns/#runner
#[allow(unused)]
#[derive(Debug)]
pub struct Runner {
    executor: Executor,
    options: RunnerOptions,
    input: Input,
    output: Output,
}

impl Runner {
    /// Configures a runner without starting it.
    ///
    /// Adds one `--define=<key>=<value>` argument per entry in `options.define`,
    /// in `BTreeMap` key order, followed by `options.other`. Duplicate keys have
    /// already been collapsed by the map; ordered or repeated definitions can
    /// instead be supplied through `options.other`. Names must be nonempty and
    /// contain no `=`; values may be empty or contain `=`. This constructor does
    /// not validate them. The input and output values select stdin and stdout;
    /// stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: Input,
        output: Output,
        options: RunnerOptions,
    ) -> Self {
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(
                &options
                    .define
                    .iter()
                    .map(|(k, v)| format!("--define={}={}", k, v))
                    .collect::<Vec<_>>(),
            )
            .args(&options.other)
            .stdin(input.as_stdio())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    /// Sends the remaining input to a new child and returns captured stdout bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning, copying input, or waiting fails,
    /// or if the runner exits unsuccessfully.
    pub async fn execute(&mut self) -> RunnerResult {
        let stdout = self.executor.execute_with_input(&mut self.input).await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Runner<Cursor<Vec<u8>>, ExecutorError> for Runner {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Runner {
    async fn execute(&mut self) -> RunnerResult {
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
