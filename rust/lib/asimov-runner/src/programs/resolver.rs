// This is free and unencumbered software released into the public domain.

//! URI-to-URL resolution through an external resolver program.
//!
//! Process execution is implemented, but extracting resolved URLs from
//! stdout is not: [`Resolver::execute`] currently returns an empty list on success.

use crate::{Executor, ExecutorError, Output};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use async_trait::async_trait;
use derive_more::Debug;
use std::{ffi::OsStr, process::Stdio};

pub use asimov_patterns::ResolverOptions;

/// A list of resolved URLs, or an execution error.
///
/// The current [`Resolver`] implementation always returns an empty vector on
/// success because stdout parsing is not yet implemented.
pub type ResolverResult = std::result::Result<Vec<String>, ExecutorError>;

/// An external [resolver] that maps a URI (a URN or URL) to resolved URLs.
///
/// The input URI is passed as one command-line argument, and stdin is connected
/// to the null device. The external program performs resolution, but this wrapper
/// discards captured stdout and returns an empty list after a successful exit.
/// Stream handling follows the behavior described in [`crate::programs`].
///
/// [resolver]: https://asimov-specs.github.io/program-patterns/#resolver
#[allow(unused)]
#[derive(Debug)]
pub struct Resolver {
    executor: Executor,
    options: ResolverOptions,
    input: String,
    output: Output,
}

impl Resolver {
    /// Configures a resolver for the URI `input` without starting it.
    ///
    /// Adds `--limit=<limit>` when `options.limit` is set, followed by
    /// `options.other` and the unvalidated URI as a single argument. `output`
    /// selects stdout handling; stderr is captured for failure diagnostics.
    pub fn new(
        program: impl AsRef<OsStr>,
        input: impl AsRef<str>,
        output: Output,
        options: ResolverOptions,
    ) -> Self {
        let input = input.as_ref().to_string();
        let mut executor = Executor::new(program);
        executor
            .command()
            .args(if let Some(limit) = options.limit {
                vec![format!("--limit={}", limit)]
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

    /// Runs a new resolver process and currently returns an empty list on success.
    ///
    /// Captured stdout is discarded rather than parsed into URLs.
    ///
    /// # Errors
    ///
    /// Returns an [`ExecutorError`] if spawning or waiting fails, or if the
    /// resolver exits unsuccessfully.
    pub async fn execute(&mut self) -> ResolverResult {
        let _stdout = self.executor.execute().await?;
        //let lines = stdout.lines().into_iter().collect::<Vec<_>>().await?; // FIXME
        Ok(Vec::new())
    }
}

impl asimov_patterns::Resolver<Vec<String>, ExecutorError> for Resolver {}

#[async_trait]
impl asimov_patterns::Execute<Vec<String>, ExecutorError> for Resolver {
    async fn execute(&mut self) -> ResolverResult {
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
