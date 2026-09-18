// This is free and unencumbered software released into the public domain.

//! RDF value generation through an external emitter program with no stdin input.

use crate::{Executor, ExecutorError, GraphOutput, JsonlStream, NoInput, Output};
use alloc::{boxed::Box, format, vec};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::EmitterOptions;

/// A live JSONL graph stream, or an error starting the emitter.
pub type EmitterResult = Result<JsonlStream, ExecutorError>;

/// An external [emitter] that generates values as RDF without reading stdin.
///
/// Stdin is connected to the null device. The program determines what data to
/// emit from its arguments, environment, and other external sources. Execution
/// uses the concurrent streaming behavior described in [`crate::programs`].
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

    /// Starts a new emitter process and returns its live JSONL graph stream.
    ///
    /// # Errors
    ///
    /// Spawn failures are returned directly; read, wait, and exit failures are stream
    /// items. Consume the stream to completion to check process success.
    pub async fn execute(&mut self) -> EmitterResult {
        self.executor.execute_jsonl().await
    }
}

impl asimov_patterns::Emitter<JsonlStream, ExecutorError> for Emitter {}

#[async_trait]
impl asimov_patterns::Execute<JsonlStream, ExecutorError> for Emitter {
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
