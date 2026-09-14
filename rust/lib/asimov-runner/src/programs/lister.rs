// This is free and unencumbered software released into the public domain.

//! URL-based directory iteration through an external lister program.

use crate::{Executor, ExecutorError, GraphOutput};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, io::Cursor, process::Stdio};

pub use asimov_patterns::ListerOptions;

/// Raw graph bytes captured from a successful [`Lister`], or an execution error.
///
/// The cursor is positioned at zero and is empty when stdout is not captured.
/// Entries are not parsed into individual values.
pub type ListerResult = std::result::Result<Cursor<Vec<u8>>, ExecutorError>; // TODO

/// An external [lister] that iterates a directory URL and emits RDF for its entries.
///
/// The input URL is passed as one command-line argument, and stdin is connected
/// to the null device. Sorting and pagination are delegated to the external
/// program via options. Execution uses the buffering and stream-handling
/// behavior described in [`crate::programs`].
///
/// [lister]: https://asimov-specs.github.io/program-patterns/#lister
#[allow(unused)]
#[derive(Debug)]
pub struct Lister {
    executor: Executor,
    options: ListerOptions,
    input: String,
    output: GraphOutput,
}

impl Lister {
    /// Configures a lister for the directory URL `input` without starting it.
    ///
    /// Adds any configured `--sort`, `--offset`, `--limit`, and `--output`
    /// options as `--name=value` arguments, in that order, followed by
    /// `options.other` and the unvalidated URL. `output` selects stdout handling;
    /// stderr is captured for failure diagnostics.
    ///
    /// The specification defines `--limit` and `--output`; support for the
    /// additional `--sort` and `--offset` arguments depends on the program.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: GraphOutput,
        options: ListerOptions,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(ref sort) = options.sort {
                vec![format!("--sort={}", sort.to_string())]
            } else {
                vec![]
            })
            .args(if let Some(offset) = options.offset {
                vec![format!("--offset={}", offset)]
            } else {
                vec![]
            })
            .args(if let Some(limit) = options.limit {
                vec![format!("--limit={}", limit)]
            } else {
                vec![]
            })
            .args(if let Some(ref output) = options.output {
                vec![format!("--output={}", output)]
            } else {
                vec![]
            })
            .args(&options.other)
            .arg(&input)
            .stdin(Stdio::null())
            .stdout(output.as_stdio())
            .stderr(Stdio::piped());

        Self {
            executor,
            options,
            input,
            output,
        }
    }

    /// Runs a new lister process and returns its captured listing bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning or waiting fails, or if the
    /// lister exits unsuccessfully.
    pub async fn execute(&mut self) -> ListerResult {
        let stdout = self.executor.execute().await?;
        Ok(stdout)
    }
}

impl asimov_patterns::Lister<Cursor<Vec<u8>>, ExecutorError> for Lister {}

#[async_trait]
impl asimov_patterns::Execute<Cursor<Vec<u8>>, ExecutorError> for Lister {
    async fn execute(&mut self) -> ListerResult {
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
